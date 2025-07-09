use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use futures::stream::StreamExt;
use libp2p::gossipsub::PublishError;
use libp2p::kad::QueryId;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, gossipsub, identify, identity, kad, noise, ping,
    relay, tcp, yamux,
};
use niketsu_core::communicator::{
    ChunkResponseMsg, FileRequestMsg, FileResponseMsg, UserMessageMsg,
};
use niketsu_core::playlist::Video;
use niketsu_core::playlist::file::PlaylistBrowser;
use niketsu_core::room::RoomName;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::{debug, info, warn};

use crate::CONNECT_TIMEOUT;
use crate::messages::NiketsuMessage;

mod client;
mod host;

static KEYPAIR: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    relay_client: relay::client::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    message_request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
    kademlia: kad::Behaviour<MemoryStore>,
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
        let swarm_response = SwarmResponse::from(msg);
        swarm_response.handle_swarm_response(self.handler())
    }
    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()>;
    fn handler(&mut self) -> &mut CommunicationHandler;
}

#[enum_dispatch(CommunicationHandlerTrait)]
pub(crate) enum Handler {
    Client(client::ClientCommunicationHandler),
    Host(host::HostCommunicationHandler),
}

trait SwarmHandler<T, TResponse> {
    fn send_request(&mut self, peer_id: &PeerId, msg: T);
    fn send_response(&mut self, channel: ResponseChannel<TResponse>, msg: TResponse) -> Result<()>;
    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: T) -> Result<()>;
    fn start_providing(&mut self, video: Video) -> Result<()>;
    fn stop_providing(&mut self, video: Video);
}

impl SwarmHandler<NiketsuMessage, MessageResponse> for Swarm<Behaviour> {
    fn send_request(&mut self, peer_id: &PeerId, msg: NiketsuMessage) {
        // ignores outbound id
        self.behaviour_mut()
            .message_request_response
            .send_request(peer_id, MessageRequest(msg));
    }

    fn send_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()> {
        let res = self
            .behaviour_mut()
            .message_request_response
            .send_response(channel, msg);

        match res {
            Ok(_) => {
                debug!("Successfully send response status");
                Ok(())
            }
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
    }

    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()> {
        // ignores message id and insufficient peer error

        let res = self
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), Vec::<u8>::try_from(msg)?);

        match res {
            Ok(_) => Ok(()),
            Err(e) => match e {
                PublishError::NoPeersSubscribedToTopic => {
                    debug!("Gossipsub insufficient peers. Publishing when no one is connected");
                    Ok(())
                }
                err => Err(anyhow::Error::from(err)),
            },
        }
    }

    fn start_providing(&mut self, video: Video) -> Result<()> {
        let filename = video.as_str().as_bytes().to_vec();
        let res = self
            .behaviour_mut()
            .kademlia
            .start_providing(filename.clone().into());

        match res {
            Ok(id) => {
                debug!(?filename, ?id, "Successfully started providing file");
                Ok(())
            }
            Err(e) => bail!("Failed to start providing {e:?}"),
        }
    }

    fn stop_providing(&mut self, video: Video) {
        let filename = video.as_str().as_bytes().to_vec();
        self.behaviour_mut()
            .kademlia
            .stop_providing(&filename.clone().into());
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
                .clone()
                .with(Protocol::P2pCircuit)
                .with(Protocol::P2p(peer_id));

            self.dial(peer_addr.clone()).unwrap();
            self.behaviour_mut()
                .kademlia
                .add_address(&peer_id, peer_addr);
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
                    self.behaviour_mut()
                        .init_request_response
                        .send_request(&peer_id, InitRequest::new(room.clone(), password.clone()));
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
                            StatusResponse::Err => {
                                bail!("Authentication failed");
                            }
                            _ => {
                                bail!("Received unexpected response from relay")
                            }
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
        self.dial(relay_addr.clone())
            .context("Failed to dial relay")?;

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
        info!(
            peer_id = %swarm.local_peer_id(),
            "Starting client with peer id",
        );

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
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    current_requests: HashMap<QueryId, FileRequestMsg>,
    pending_request_provider: Option<PeerId>,
    pending_chunk_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    pending_file_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    current_response: Option<Video>,
}

impl CommunicationHandler {
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
            current_requests: Default::default(),
            pending_chunk_responses: Default::default(),
            pending_file_responses: Default::default(),
            pending_request_provider: None,
            current_response: None,
        }
    }
}

#[enum_dispatch()]
trait SwarmResponseHandler {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()>;
}

#[enum_dispatch(SwarmResponseHandler)]
enum SwarmResponse {
    Status(StatusResponse),
    ChunkResponse(ChunkResponseMsg),
    FileResponse(FileResponseMsg),
    Other(NiketsuMessage),
}

impl SwarmResponse {
    fn from(message: MessageResponse) -> Self {
        match message.0 {
            Response::Status(msg) => SwarmResponse::Status(msg),
            Response::Message(niketsu_message) => match niketsu_message {
                NiketsuMessage::FileResponse(msg) => SwarmResponse::FileResponse(msg),
                NiketsuMessage::ChunkResponse(msg) => SwarmResponse::ChunkResponse(msg),
                msg => SwarmResponse::Other(msg),
            },
        }
    }
}

impl SwarmResponseHandler for StatusResponse {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        debug!(?self, "Received status response");
        if self == StatusResponse::NotProvidingErr {
            handler.pending_request_provider.take();
        }
        Ok(())
    }
}

impl SwarmResponseHandler for ChunkResponseMsg {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        handler
            .message_sender
            .send(self.clone().into())
            .map_err(anyhow::Error::from)
    }
}

impl SwarmResponseHandler for FileResponseMsg {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        handler
            .message_sender
            .send(self.clone().into())
            .map_err(anyhow::Error::from)
    }
}

impl SwarmResponseHandler for NiketsuMessage {
    fn handle_swarm_response(self, _handler: &mut CommunicationHandler) -> Result<()> {
        bail!("Did not expect response {self:?}");
    }
}

#[enum_dispatch]
pub(crate) trait SwarmEventHandler {
    fn handle_swarm_event(self, handler: &mut CommunicationHandler);
}

impl SwarmEventHandler for kad::Event {
    fn handle_swarm_event(self, handler: &mut CommunicationHandler) {
        match self {
            kad::Event::OutboundQueryProgressed {
                id,
                result:
                    kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {
                        providers,
                        ..
                    })),
                ..
            } => match handler.current_requests.get(&id) {
                Some(request) => {
                    if let Some(provider) = handler.pending_request_provider {
                        debug!("Already have provider");
                        handler
                            .swarm
                            .behaviour_mut()
                            .message_request_response
                            .send_request(
                                &provider,
                                MessageRequest(NiketsuMessage::FileRequest(request.clone())),
                            );
                    } else if let Some(provider) = providers.iter().next() {
                        debug!("Found providers");
                        handler.pending_request_provider = Some(*provider);

                        handler
                            .swarm
                            .behaviour_mut()
                            .message_request_response
                            .send_request(
                                provider,
                                MessageRequest(NiketsuMessage::FileRequest(request.clone())),
                            );
                    }
                }
                None => warn!("Found providers but no request?"),
            },
            kad::Event::OutboundQueryProgressed {
                result:
                    kad::QueryResult::GetProviders(Ok(
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. },
                    )),
                ..
            } => {
                debug!("No kademlia providers found");
                if let Err(err) =
                    handler
                        .message_sender
                        .send(NiketsuMessage::UserMessage(UserMessageMsg {
                            actor: const { arcstr::literal!("server") },
                            message: "No providers found for the requested file".into(),
                        }))
                {
                    debug!(?err, "Failed to send message to core");
                }
            }
            kad_event => debug!(?kad_event, "Received non handled kademlia event"),
        }
    }
}
