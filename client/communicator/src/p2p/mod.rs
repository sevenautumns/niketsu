use std::ops::{Deref, DerefMut};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use file_share::{FileShare, FileShareRequest, FileShareResponseResult};
use futures::stream::StreamExt;
use libp2p::gossipsub::PublishError;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, gossipsub, identify, identity, kad, mdns, noise,
    ping, relay, tcp, yamux,
};
use niketsu_core::communicator::UserMessageMsg;
use niketsu_core::playlist::Video;
use niketsu_core::playlist::file::PlaylistBrowser;
use niketsu_core::room::RoomName;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::{debug, info};

use crate::CONNECT_TIMEOUT;
use crate::messages::NiketsuMessage;

mod client;
mod file_share;
mod host;

static KEYPAIR: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    relay_client: relay::client::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    fileshare_request_response:
        request_response::cbor::Behaviour<FileShareRequest, FileShareResponseResult>,
    message_request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
    kademlia: kad::Behaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
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
enum StatusResponse {
    Ok,
    Err,
    NotProvidingErr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitRequest {
    room: RoomName,
    password: String,
}

impl InitRequest {
    fn new(room: RoomName, password: String) -> Self {
        Self {
            room,
            password: digest(password),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: StatusResponse,
    peer_id: Option<PeerId>, // peer id of room if found
}

#[async_trait]
#[enum_dispatch]
pub(crate) trait CommunicationHandlerTrait {
    async fn run(&mut self);
    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>);
    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()>;
    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()>;
    fn handle_swarm_response(&mut self, msg: MessageResponse, peer_id: PeerId) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Received response");
        match msg.0 {
            Response::Message(niketsu_message) => match niketsu_message {
                NiketsuMessage::FileResponse(_) | NiketsuMessage::ChunkResponse(_) => Ok(()),
                msg => bail!("Did not expect response {msg:?}"),
            },
            _ => Ok(()),
        }
    }
    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()>;
}

#[enum_dispatch(CommunicationHandlerTrait)]
pub(crate) enum Handler {
    Client(client::ClientCommunicationHandler),
    Host(host::HostCommunicationHandler),
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
        let req_resp = &mut self.behaviour_mut().message_request_response;
        req_resp.send_request(peer_id, MessageRequest(msg))
    }

    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId {
        // ignores outbound id
        let req_resp = &mut self.behaviour_mut().fileshare_request_response;
        req_resp.send_request(peer_id, msg)
    }

    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().message_request_response;
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
        let req_resp = &mut self.behaviour_mut().fileshare_request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
    }

    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()> {
        // ignores message id and insufficient peer error

        let gossip = &mut self.behaviour_mut().gossipsub;
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
        let kad = &mut self.behaviour_mut().kademlia;
        let res = kad.start_providing(filename.clone().into());

        match res {
            Ok(id) => debug!(?filename, ?id, "Successfully started providing file"),
            Err(e) => bail!("Failed to start providing {e:?}"),
        }
        Ok(())
    }

    fn stop_providing(&mut self, video: &Video) {
        let filename = video.as_str().as_bytes().to_vec();
        let kad = &mut self.behaviour_mut().kademlia;
        kad.stop_providing(&filename.clone().into());
        debug!(?filename, "Stopped providing file");
    }
}

#[async_trait]
trait SwarmRelayConnection {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<Host>;
    async fn identify_loop(&mut self, room: RoomName, password: String) -> Result<PeerInfo>;
    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<PeerInfo>;
}

type Host = PeerId;

#[derive(Debug)]
struct PeerInfo {
    relay: PeerId,
    host: Option<Host>,
}

#[async_trait]
impl SwarmRelayConnection for Swarm<Behaviour> {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<Host> {
        self.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        self.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        self.listen_on("/ip6/::/udp/0/quic-v1".parse()?)?;
        self.listen_on("/ip6/::/tcp/0".parse()?)?;

        let peer_info = self
            .identify_relay(relay_addr.clone(), room, password)
            .await?;

        let relay_addr = relay_addr
            .with_p2p(peer_info.relay)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        debug!(?peer_info, "Peer IDs from relay");
        if let Some(peer_id) = peer_info.host {
            info!(%peer_id, "Dialing peer");
            let peer_addr = relay_addr
                .with(Protocol::P2pCircuit)
                .with(Protocol::P2p(peer_id));

            self.dial(peer_addr.clone()).unwrap();
            let kad = &mut self.behaviour_mut().kademlia;
            kad.add_address(&peer_id, peer_addr);
            return Ok(peer_id);
        }

        info!(?relay_addr, "Initialization successful. Listening on relay");
        self.listen_on(relay_addr.clone().with(Protocol::P2pCircuit))
            .expect("Failed to listen on remote relay");

        Ok(*self.local_peer_id())
    }

    async fn identify_loop(&mut self, room: RoomName, password: String) -> Result<PeerInfo> {
        let mut host_peer_id: Option<PeerId> = None;
        let mut relay_peer_id: Option<PeerId> = None;
        let mut learned_observed_addr = false;
        let mut told_relay_observed_addr = false;
        let mut learned_host_peer_id = false;
        let mut learned_relay_peer_id = false;

        loop {
            match self.next().await.unwrap() {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Sent {
                    ..
                })) => {
                    debug!("Told relay its public address");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    peer_id,
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    debug!(%observed_addr, "Relay told us our observed address");
                    debug!("Sending new room request to relay");
                    let req_resp = &mut self.behaviour_mut().init_request_response;
                    let req = InitRequest::new(room.clone(), password.clone());
                    req_resp.send_request(&peer_id, req);
                    learned_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { message, .. },
                )) => {
                    if let request_response::Message::Response { response, .. } = message {
                        match response.status {
                            StatusResponse::Ok => {
                                host_peer_id = response.peer_id;
                                learned_host_peer_id = true;
                            }
                            StatusResponse::Err => bail!("Authentication failed"),
                            _ => bail!("Received unexpected response from relay"),
                        }
                    }
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    debug!(%peer_id, "Learned relay peer id");
                    relay_peer_id = Some(peer_id);
                    learned_relay_peer_id = true;
                }
                event => debug!(?event, "Received other relay events"),
            }

            if learned_observed_addr
                && told_relay_observed_addr
                && learned_host_peer_id
                && learned_relay_peer_id
            {
                break;
            }
        }

        Ok(PeerInfo {
            relay: relay_peer_id.unwrap(),
            host: host_peer_id,
        })
    }

    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<PeerInfo> {
        info!("Dialing relay for identify exchange");
        self.dial(relay_addr).context("Failed to dial relay")?;

        // time out is already set in the communicator, so this could be dropped
        let host_peer_id =
            tokio::time::timeout(CONNECT_TIMEOUT, self.identify_loop(room, password)).await;

        host_peer_id.context("Identify exchange with relay timed out")?
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

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
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
                    relay_client: relay_behaviour,
                    ping: ping::Behaviour::new(
                        ping::Config::new().with_interval(Duration::from_secs(1)),
                    ),
                    identify: identify::Behaviour::new(identify::Config::new(
                        "/niketsu-identify/1".to_string(),
                        key.public(),
                    )),
                    dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
                    gossipsub: gossipsub::Behaviour::new(
                        gossipsub::MessageAuthenticity::Signed(key.clone()),
                        gossipsub_config,
                    )?,
                    message_request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/niketsu-message/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default()
                            .with_request_timeout(Duration::from_secs(5)),
                    ),
                    fileshare_request_response: request_response::cbor::Behaviour::new(
                        [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                        request_response::Config::default()
                            .with_request_timeout(Duration::from_secs(5)),
                    ),
                    init_request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/authorisation/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default()
                            .with_request_timeout(Duration::from_secs(10)),
                    ),
                    kademlia: kad::Behaviour::new(
                        keypair.public().to_peer_id(),
                        MemoryStore::new(key.public().to_peer_id()),
                    ),
                    mdns: mdns::tokio::Behaviour::new(
                        mdns::Config::default(),
                        key.public().to_peer_id(),
                    )?,
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
            .build();

        debug!(%relay_addr, "Attempting to connect to relay");

        let room2 = room.clone();
        let playlist_handler =
            tokio::task::spawn(async move { PlaylistBrowser::get_first(&room2).await });

        let host = swarm
            .establish_conection(relay_addr.clone(), room.clone(), password.clone())
            .await?;
        info!(peer_id = %swarm.local_peer_id(), "Starting client with peer id");

        let topic_hash = digest(format!("{room}|{password}"));
        let topic = gossipsub::IdentTopic::new(topic_hash); // can topics be discovered of new nodes?
        swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

        let (core_sender, core_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (message_sender, message_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut handler: Handler = if host == (*swarm.local_peer_id()) {
            Handler::Host(host::HostCommunicationHandler::new(
                swarm,
                topic,
                host,
                relay_addr.clone(),
                core_receiver,
                message_sender,
                room,
                playlist_handler.await.ok().flatten().unwrap_or_default(),
            ))
        } else {
            Handler::Client(client::ClientCommunicationHandler::new(
                swarm,
                topic,
                host,
                relay_addr.clone(),
                core_receiver,
                message_sender,
            ))
        };

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

impl NiketsuMessage {
    fn broadcast(self, handler: &mut CommunicationHandler) -> Result<()> {
        let topic = handler.topic.clone();
        handler.swarm.try_broadcast(topic, self)
    }

    fn respond_with_err(
        self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let resp = MessageResponse(Response::Status(StatusResponse::Err));
        handler.swarm.send_message_response(channel, resp)?;
        bail!("Host received unexpected direct message: {self:?}");
    }

    fn send_to_core(
        self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let resp = match handler.message_sender.send(self.clone()) {
            Ok(_) => MessageResponse(Response::Status(StatusResponse::Ok)),
            Err(_) => MessageResponse(Response::Status(StatusResponse::Err)),
        };
        handler.swarm.send_message_response(channel, resp)
    }
}
