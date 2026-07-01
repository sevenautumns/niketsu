use std::pin::Pin;
use std::time::Duration;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use file_share::{FileShare, FileShareRequest, FileShareResponseResult};
use futures::StreamExt;
use libp2p::gossipsub::PublishError;
use libp2p::kad::store::MemoryStore;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, gossipsub, identify, identity, kad, mdns, noise,
    ping, relay, tcp, yamux,
};
use niketsu_core::communicator::UserMessageMsg;
use niketsu_core::log_err_msg;
use niketsu_core::playlist::Video;
use niketsu_core::room::RoomName;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::{debug, error, trace, warn};

use crate::messages::NiketsuMessage;

mod auth;
mod connecting;
mod direct_only;
mod host_relay;

mod client;
mod file_share;
mod host;
mod room;

use direct_only::DirectOnly;

static KEYPAIR: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);

/// How long a client waits for a *direct* connection to the host (via DCUtR
/// hole punching or a mDNS LAN dial) before giving up and reconnecting. The
/// relay circuit only carries hole-punch coordination, so until this succeeds
/// the client is not considered connected. Only the client arms this deadline;
/// the host leaves it unset.
pub(crate) const DIRECT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Result of one iteration of the shared event loop in
/// [`CommunicationHandlerTrait::run`]. Short-lived stack value, consumed
/// immediately each iteration; boxing the swarm event would add a heap
/// allocation on the hottest path for no real benefit.
#[allow(clippy::large_enum_variant)]
enum LoopEvent {
    Swarm(SwarmEvent<BehaviourEvent>),
    Core(Option<NiketsuMessage>),
    /// The client's direct-connection deadline elapsed; abort the loop so the
    /// core reconnects.
    Abort,
}

/// Awaits the deadline if one is armed, otherwise never resolves. Kept as a
/// free function so it borrows only the deadline field, leaving the rest of
/// `CommonCommunication` free to borrow in the same `select!`.
async fn wait_deadline(deadline: &mut Option<Pin<Box<tokio::time::Sleep>>>) {
    match deadline {
        Some(sleep) => sleep.as_mut().await,
        None => std::future::pending().await,
    }
}

/// Connection setup: relay, identify, hole-punching, keepalive, and room auth.
/// `relay_server` is disabled by default and only enabled when this peer is the room host.
#[derive(NetworkBehaviour)]
pub(crate) struct TransportBehaviour {
    relay_client: relay::client::Behaviour,
    relay_server: host_relay::GatedRelayBehaviour,
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
    messaging: DirectOnly<MessagingBehaviour>,
    file_share: DirectOnly<FileShareBehaviour>,
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

/// Common event loop and dispatch for the host and client roles. The
/// provided methods handle everything both roles share (gossip broadcasts,
/// direct messages, file sharing); implementors supply the role-specific
/// message handling and the residual swarm events.
#[async_trait]
pub(crate) trait CommunicationHandlerTrait: Send {
    fn handler_mut(&mut self) -> &mut CommunicationHandler;
    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()>;
    fn handle_swarm_broadcast(&mut self, data: Vec<u8>, source: PeerId) -> Result<()>;
    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()>;
    /// Role-specific swarm events not covered by `dispatch_swarm_event`.
    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>);

    /// Request-response events other than `Message` (role-specific logging).
    fn on_msg_req_resp_other(
        &mut self,
        event: request_response::Event<MessageRequest, MessageResponse>,
    ) {
        debug!(
            ?event,
            "Received request response event that is not handled"
        );
    }

    async fn run(&mut self) {
        loop {
            let next = {
                let base = &mut self.handler_mut().base;
                tokio::select! {
                    event = base.swarm.select_next_some() => LoopEvent::Swarm(event),
                    msg = base.core_receiver.recv() => LoopEvent::Core(msg),
                    _ = wait_deadline(&mut base.direct_connection_deadline) => LoopEvent::Abort,
                }
            };
            match next {
                LoopEvent::Swarm(event) => self.dispatch_swarm_event(event),
                LoopEvent::Core(Some(msg)) => {
                    debug!(?msg, "Handling core message");
                    let res = self.handle_core_message(msg);
                    log_err_msg!(res, "Handling message caused error");
                }
                LoopEvent::Core(None) => {
                    error!("Channel of core closed. Stopping p2p event loop");
                    break;
                }
                LoopEvent::Abort => {
                    warn!("No direct connection to host within timeout. Stopping p2p event loop");
                    break;
                }
            }
        }
    }

    fn dispatch_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        debug!(?event, "Handling event from swarm");
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        use MessagingBehaviourEvent as M;
        match event {
            SwarmEvent::Behaviour(Messaging(M::Gossipsub(e))) => self.on_gossipsub(e),
            SwarmEvent::Behaviour(Messaging(M::RequestResponse(e))) => self.on_msg_req_resp(e),
            SwarmEvent::Behaviour(FileShare(F::RequestResponse(e))) => {
                self.handler_mut().handle_file_share_req_resp_event(e)
            }
            SwarmEvent::Behaviour(FileShare(F::Kademlia(e))) => {
                self.handler_mut().handle_file_share_kad_event(e)
            }
            other => self.handle_swarm_event(other),
        }
    }

    fn on_gossipsub(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                message_id,
                message,
                ..
            } => {
                debug!(%message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                // propagation_source is only the last hop in the gossip mesh;
                // the signed source identifies the actual author.
                let Some(source) = message.source else {
                    warn!(%message_id, "Dropping gossipsub message without signed source");
                    return;
                };
                let res = self.handle_swarm_broadcast(message.data, source);
                log_err_msg!(res, "Failed to handle broadcast message");
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }

    fn on_msg_req_resp(&mut self, event: request_response::Event<MessageRequest, MessageResponse>) {
        match event {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = self.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    let res = self.handler_mut().handle_swarm_response(response, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
            },
            other => self.on_msg_req_resp_other(other),
        }
    }
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
        let req_resp = &mut self.behaviour_mut().messaging.inner_mut().request_response;
        req_resp.send_request(peer_id, MessageRequest(msg))
    }

    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId {
        // ignores outbound id
        let req_resp = &mut self.behaviour_mut().file_share.inner_mut().request_response;
        req_resp.send_request(peer_id, msg)
    }

    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().messaging.inner_mut().request_response;
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
        let req_resp = &mut self.behaviour_mut().file_share.inner_mut().request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
    }

    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()> {
        // ignores message id and insufficient peer error

        let gossip = &mut self.behaviour_mut().messaging.inner_mut().gossipsub;
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
        let kad = &mut self.behaviour_mut().file_share.inner_mut().kademlia;
        let res = kad.start_providing(filename.clone().into());

        match res {
            Ok(id) => debug!(?filename, ?id, "Successfully started providing file"),
            Err(e) => bail!("Failed to start providing {e:?}"),
        }
        Ok(())
    }

    fn stop_providing(&mut self, video: &Video) {
        let filename = video.as_str().as_bytes().to_vec();
        let kad = &mut self.behaviour_mut().file_share.inner_mut().kademlia;
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
        quic_config.max_idle_timeout = 5 * 1000;

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

                // The host's relay circuits (client↔client via
                // `/p2p/HOST/p2p-circuit/p2p/PEER`) are only a hole-punch
                // rendezvous: DirectOnly keeps messaging and file_share off
                // relayed connections, so nothing but identify/DCUtR/ping ever
                // flows here. File requests time out after 30 s, so any older
                // circuit is dead weight; the duration cap only needs to stay
                // above that punch budget. The byte cap guards against peers
                // that don't play by DirectOnly's rules.
                let relay_server_config = relay::Config {
                    max_reservations: 32,
                    max_reservations_per_peer: 1,
                    max_circuits: 64,
                    max_circuits_per_peer: 4,
                    reservation_duration: Duration::from_secs(3600),
                    max_circuit_duration: Duration::from_secs(60),
                    max_circuit_bytes: 128 * 1024,
                    ..Default::default()
                };

                Ok(Behaviour {
                    transport: TransportBehaviour {
                        relay_client: relay_behaviour,
                        relay_server: host_relay::GatedRelayBehaviour::new(
                            key.public().to_peer_id(),
                            relay_server_config,
                        ),
                        identify: identify::Behaviour::new(identify::Config::new(
                            "/niketsu-identify/1".to_string(),
                            key.public(),
                        )),
                        dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
                        ping: ping::Behaviour::new(
                            ping::Config::new()
                                .with_interval(Duration::from_secs(1))
                                .with_timeout(Duration::from_secs(3)),
                        ),
                        auth: auth::AuthBehaviour::new(),
                    },
                    messaging: DirectOnly::new(MessagingBehaviour {
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
                    }),
                    file_share: DirectOnly::new(FileShareBehaviour {
                        request_response: request_response::cbor::Behaviour::new(
                            [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                            request_response::Config::default()
                                .with_request_timeout(Duration::from_secs(30)),
                        ),
                        // Kademlia stays in its default auto mode: it answers
                        // queries (server mode) only once the swarm confirms an
                        // external address, and the sole source of confirmation
                        // here is an accepted relay reservation (the host
                        // reserves on the relay server at startup, clients on
                        // the host's circuit). Drop those reservations and
                        // provider discovery silently dies because every node
                        // stays a kad client.
                        kademlia: kad::Behaviour::new(
                            keypair.public().to_peer_id(),
                            MemoryStore::new(key.public().to_peer_id()),
                        ),
                        mdns: mdns::tokio::Behaviour::new(
                            mdns::Config::default(),
                            key.public().to_peer_id(),
                        )?,
                    }),
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
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        let base = CommonCommunication::new(swarm, topic, host, core_receiver, message_sender);
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
        let topic = self.base.topic.clone();
        self.base.swarm.try_broadcast(topic, msg)
    }

    pub fn respond_with_err(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = MessageResponse(Response::Status(StatusResponse::Err));
        self.base.swarm.send_message_response(channel, resp)?;
        bail!("Received unexpected direct message: {msg:?}");
    }

    pub fn send_to_core(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = match self.base.message_sender.send(msg.clone()) {
            Ok(_) => MessageResponse(Response::Status(StatusResponse::Ok)),
            Err(_) => MessageResponse(Response::Status(StatusResponse::Err)),
        };
        self.base.swarm.send_message_response(channel, resp)
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

pub(crate) struct CommonCommunication {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    /// Deadline for getting a direct connection to the host. Only the client
    /// arms it (see [`arm_direct_deadline`]); the host leaves it `None` so the
    /// shared run loop never aborts for the host.
    direct_connection_deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl CommonCommunication {
    pub fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        Self {
            swarm,
            topic,
            host,
            core_receiver,
            message_sender,
            direct_connection_deadline: None,
        }
    }

    /// Arms the direct-connection deadline, after which the run loop aborts
    /// (and the core reconnects) unless a direct connection to the host is
    /// established first.
    pub fn arm_direct_deadline(&mut self, timeout: Duration) {
        self.direct_connection_deadline = Some(Box::pin(tokio::time::sleep(timeout)));
    }

    /// Disarms the deadline once a direct connection to the host exists.
    pub fn clear_direct_deadline(&mut self) {
        self.direct_connection_deadline = None;
    }

    pub fn send_chat_message(&self, actor: ArcStr, message: String) -> Result<()> {
        let msg = UserMessageMsg { actor, message };
        self.message_sender.send(NiketsuMessage::UserMessage(msg))?;
        Ok(())
    }
}
