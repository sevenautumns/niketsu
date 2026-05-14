use std::ops::{Deref, DerefMut};
use std::time::Duration;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use file_share::{FileShare, FileShareRequest, FileShareResponseResult};
use libp2p::gossipsub::PublishError;
use libp2p::kad::store::MemoryStore;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, gossipsub, identify, identity, kad, mdns, noise,
    ping, relay, tcp, yamux,
};
use niketsu_core::communicator::UserMessageMsg;
use niketsu_core::playlist::Video;
use niketsu_core::room::RoomName;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::debug;

use crate::messages::NiketsuMessage;

mod auth;
mod connecting;

mod client;
mod file_share;
mod host;

static KEYPAIR: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);

/// Connection setup: relay, identify, hole-punching, keepalive, and room auth.
#[derive(NetworkBehaviour)]
pub(crate) struct TransportBehaviour {
    relay_client: relay::client::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    ping: ping::Behaviour,
    auth: auth::AuthBehaviour,
}

/// Room sync protocol: gossip broadcasts and direct host↔client messages.
#[derive(NetworkBehaviour)]
pub(crate) struct MessagingBehaviour {
    gossipsub: gossipsub::Behaviour,
    request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
}

/// P2P file sharing: provider discovery (kademlia + mDNS) and chunk transfer.
#[derive(NetworkBehaviour)]
pub(crate) struct FileShareBehaviour {
    request_response: request_response::cbor::Behaviour<FileShareRequest, FileShareResponseResult>,
    kademlia: kad::Behaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
}

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    transport: TransportBehaviour,
    messaging: MessagingBehaviour,
    file_share: FileShareBehaviour,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageRequest(NiketsuMessage);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageResponse(Response);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Response {
    Status(StatusResponse),
    Message(NiketsuMessage),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum StatusResponse {
    Ok,
    Err,
    NotProvidingErr,
}

#[async_trait]
pub(crate) trait CommunicationHandlerTrait: Send {
    async fn run(&mut self);
    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>);
    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()>;
}

trait SwarmHandler {
    fn send_request(&mut self, peer_id: &PeerId, msg: NiketsuMessage) -> OutboundRequestId;
    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId;
    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()>;
    fn send_file_response(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        msg: FileShareResponseResult,
    ) -> Result<()>;
    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()>;
    fn start_providing(&mut self, video: &Video) -> Result<()>;
    fn stop_providing(&mut self, video: &Video);
}

impl SwarmHandler for Swarm<Behaviour> {
    fn send_request(&mut self, peer_id: &PeerId, msg: NiketsuMessage) -> OutboundRequestId {
        // ignores outbound id
        let req_resp = &mut self.behaviour_mut().messaging.request_response;
        req_resp.send_request(peer_id, MessageRequest(msg))
    }

    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId {
        // ignores outbound id
        let req_resp = &mut self.behaviour_mut().file_share.request_response;
        req_resp.send_request(peer_id, msg)
    }

    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().messaging.request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
    }

    fn send_file_response(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        msg: FileShareResponseResult,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().file_share.request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
    }

    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()> {
        // ignores message id and insufficient peer error

        let gossip = &mut self.behaviour_mut().messaging.gossipsub;
        let res = gossip.publish(topic.clone(), Vec::<u8>::try_from(msg)?);

        match res {
            Err(PublishError::NoPeersSubscribedToTopic) => {
                debug!("Gossipsub insufficient peers. Publishing when no one is connected")
            }
            Err(err) => return Err(anyhow::Error::from(err)),
            _ => {}
        }
        Ok(())
    }

    fn start_providing(&mut self, video: &Video) -> Result<()> {
        let filename = video.as_str().as_bytes().to_vec();
        let kad = &mut self.behaviour_mut().file_share.kademlia;
        let res = kad.start_providing(filename.clone().into());

        match res {
            Ok(id) => debug!(?filename, ?id, "Successfully started providing file"),
            Err(e) => bail!("Failed to start providing {e:?}"),
        }
        Ok(())
    }

    fn stop_providing(&mut self, video: &Video) {
        let filename = video.as_str().as_bytes().to_vec();
        let kad = &mut self.behaviour_mut().file_share.kademlia;
        kad.stop_providing(&filename.clone().into());
        debug!(?filename, "Stopped providing file");
    }
}

#[derive(Debug)]
pub(crate) struct P2PClient {
    sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    _handler: tokio::task::JoinHandle<()>,
}

impl P2PClient {
    pub(crate) async fn new(
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<P2PClient> {
        let keypair = KEYPAIR.clone();
        let mut quic_config = libp2p::quic::Config::new(&keypair.clone());
        quic_config.handshake_timeout = Duration::from_secs(10);
        quic_config.max_idle_timeout = 10 * 1000;

        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                libp2p::noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic_config(|_| quic_config)
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_behaviour| {
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .duplicate_cache_time(Duration::from_secs(60))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(anyhow::Error::from)?;

                Ok(Behaviour {
                    transport: TransportBehaviour {
                        relay_client: relay_behaviour,
                        identify: identify::Behaviour::new(identify::Config::new(
                            "/niketsu-identify/1".to_string(),
                            key.public(),
                        )),
                        dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
                        ping: ping::Behaviour::new(
                            ping::Config::new().with_interval(Duration::from_secs(1)),
                        ),
                        auth: auth::AuthBehaviour::new(),
                    },
                    messaging: MessagingBehaviour {
                        gossipsub: gossipsub::Behaviour::new(
                            gossipsub::MessageAuthenticity::Signed(key.clone()),
                            gossipsub_config,
                        )?,
                        request_response: request_response::cbor::Behaviour::new(
                            [(
                                StreamProtocol::new("/niketsu-message/1"),
                                ProtocolSupport::Full,
                            )],
                            request_response::Config::default()
                                .with_request_timeout(Duration::from_secs(5)),
                        ),
                    },
                    file_share: FileShareBehaviour {
                        request_response: request_response::cbor::Behaviour::new(
                            [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                            request_response::Config::default()
                                .with_request_timeout(Duration::from_secs(30)),
                        ),
                        kademlia: kad::Behaviour::new(
                            keypair.public().to_peer_id(),
                            MemoryStore::new(key.public().to_peer_id()),
                        ),
                        mdns: mdns::tokio::Behaviour::new(
                            mdns::Config::default(),
                            key.public().to_peer_id(),
                        )?,
                    },
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
            .build();

        debug!(%relay_addr, "Attempting to connect to relay");

        let topic_hash = digest(format!("{room}|{password}"));
        let topic = gossipsub::IdentTopic::new(topic_hash);

        let (core_sender, core_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (message_sender, message_receiver) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = connecting::ConnectingHandler::new(
            swarm,
            relay_addr,
            room,
            password,
            topic,
            core_receiver,
            message_sender,
        )
        .run()
        .await?;

        let client = P2PClient {
            sender: core_sender,
            receiver: message_receiver,
            _handler: spawn(async move { handler.run().await }),
        };

        Ok(client)
    }

    pub(crate) async fn next(&mut self) -> Option<NiketsuMessage> {
        self.receiver.recv().await
    }

    pub(crate) fn send(&self, msg: NiketsuMessage) -> Result<()> {
        debug!(?msg, "Sending message");
        Ok(self.sender.send(msg)?)
    }
}

pub(crate) struct CommunicationHandler {
    base: CommonCommunication,
    file_share: Option<FileShare>,
}

impl CommunicationHandler {
    pub fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_addr: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        let base = CommonCommunication::new(
            swarm,
            topic,
            host,
            relay_addr,
            core_receiver,
            message_sender,
        );
        Self {
            base,
            file_share: None,
        }
    }

    fn reset_requests_responses(&mut self) {
        if let Some(FileShare::Provider(provider)) = &self.file_share {
            self.base.swarm.stop_providing(provider.video());
        }
        self.file_share.take();
    }

    pub fn handle_file_share_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        use NiketsuMessage::*;
        match msg {
            FileRequest(msg) => self.fs_file_request(msg),
            FileResponse(msg) => self.fs_file_response(msg),
            ChunkRequest(msg) => self.fs_chunk_request(msg),
            ChunkResponse(msg) => self.fs_chunk_response(msg),
            VideoShare(msg) => self.fs_video_share(msg),
            _ => unreachable!("handle_file_share_core_message called with non-file-share message"),
        }
    }

    pub fn broadcast(&mut self, msg: NiketsuMessage) -> Result<()> {
        let topic = self.topic.clone();
        self.swarm.try_broadcast(topic, msg)
    }

    pub fn respond_with_err(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = MessageResponse(Response::Status(StatusResponse::Err));
        self.swarm.send_message_response(channel, resp)?;
        bail!("Received unexpected direct message: {msg:?}");
    }

    pub fn send_to_core(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = match self.message_sender.send(msg.clone()) {
            Ok(_) => MessageResponse(Response::Status(StatusResponse::Ok)),
            Err(_) => MessageResponse(Response::Status(StatusResponse::Err)),
        };
        self.swarm.send_message_response(channel, resp)
    }

    pub fn handle_swarm_response(&self, msg: MessageResponse, peer_id: PeerId) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Received response");
        match msg.0 {
            Response::Message(niketsu_message) => match niketsu_message {
                NiketsuMessage::FileResponse(_) | NiketsuMessage::ChunkResponse(_) => Ok(()),
                msg => bail!("Did not expect response {msg:?}"),
            },
            _ => Ok(()),
        }
    }
}

impl Deref for CommunicationHandler {
    type Target = CommonCommunication;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for CommunicationHandler {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

pub(crate) struct CommonCommunication {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    relay_addr: Multiaddr,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
}

impl CommonCommunication {
    pub fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_addr: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        Self {
            swarm,
            topic,
            host,
            relay_addr,
            core_receiver,
            message_sender,
        }
    }

    pub fn send_chat_message(&self, actor: ArcStr, message: String) -> Result<()> {
        let msg = UserMessageMsg { actor, message };
        self.message_sender.send(NiketsuMessage::UserMessage(msg))?;
        Ok(())
    }
}
