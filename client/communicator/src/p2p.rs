use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use arcstr::ArcStr;
use async_trait::async_trait;
use bcrypt::{hash, DEFAULT_COST};
use enum_dispatch::enum_dispatch;
use fake::faker::company::en::Buzzword;
use fake::Fake;
use futures::StreamExt;
use libp2p::gossipsub::PublishError;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{ConnectionId, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    dcutr, gossipsub, identify, identity, noise, ping, relay, tcp, yamux, Multiaddr, PeerId,
    StreamProtocol,
};
use niketsu_core::communicator::{
    ConnectedMsg, PlaylistMsg, SelectMsg, StartMsg, UserStatusListMsg, VideoStatusMsg,
};
use niketsu_core::playlist::file::PlaylistBrowser;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::room::RoomName;
use niketsu_core::user::UserStatus;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::{debug, error, info, warn};

use crate::messages::NiketsuMessage;

static KEYPAIR: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
    message_request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MessageRequest(NiketsuMessage);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageResponse(ResponseStatus);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ResponseStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    // should be hashed when listening
    room: RoomName,
    password: String,
}

impl InitRequest {
    fn new(room: RoomName, password: String) -> Self {
        Self {
            room,
            password: hash(password, DEFAULT_COST).expect("Failed to hash password"),
        }
    }

    fn new_without_hash(room: RoomName, password: String) -> Self {
        Self { room, password }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: u8,              // 0 -> ok, 1 -> err
    peer_id: Option<PeerId>, // peer id of room if found
}

#[async_trait]
trait SwarmRelayConnection {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<Host>;
    async fn identify_loop(&mut self, room: RoomName, password: String) -> PeerInfo;
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
            self.dial(
                relay_addr
                    .clone()
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(peer_id)),
            )
            .unwrap();
            return Ok(peer_id);
        }

        info!(?relay_addr, "Initialization successful. Listening on relay");
        self.listen_on(relay_addr.clone().with(Protocol::P2pCircuit))
            .expect("Failed to listen on remote relay");

        Ok(*self.local_peer_id())
    }

    async fn identify_loop(&mut self, room: RoomName, password: String) -> PeerInfo {
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
                    request_response::Event::Message { peer, message },
                )) => {
                    if let request_response::Message::Response { response, .. } = message {
                        if response.status != 0 {
                            self.behaviour_mut().init_request_response.send_request(
                                &peer,
                                InitRequest::new_without_hash(room.clone(), password.clone()),
                            );
                        } else {
                            host_peer_id = response.peer_id;
                            learned_host_peer_id = true;
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

        PeerInfo {
            relay: relay_peer_id.unwrap(),
            host: host_peer_id,
        }
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

        let host_peer_id =
            tokio::time::timeout(Duration::from_secs(15), self.identify_loop(room, password)).await;

        host_peer_id.context("Identify exchange with relay timed out")
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

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                libp2p::noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic_config(|_| quic_config)
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|keypair, relay_behaviour| {
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
                        keypair.public(),
                    )),
                    dcutr: dcutr::Behaviour::new(keypair.public().to_peer_id()),
                    gossipsub: gossipsub::Behaviour::new(
                        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
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
            Handler::Host(HostCommunicationHandler::new(
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
            Handler::Client(ClientCommunicationHandler::new(
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

#[async_trait]
#[enum_dispatch]
trait CommunicationHandler {
    async fn run(&mut self);
    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>);
    async fn handle_message(&mut self, msg: NiketsuMessage) -> Result<()>;
}

#[enum_dispatch(CommunicationHandler)]
enum Handler {
    Client(ClientCommunicationHandler),
    Host(HostCommunicationHandler),
}

struct ClientCommunicationHandler {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    host_conn: Option<ConnectionId>,
    relay_conn: Option<ConnectionId>,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    video_status: VideoStatusMsg,
    is_seeking: bool,
    delay: Duration,
}

impl fmt::Debug for ClientCommunicationHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientCommunicationHandler")
            .field("topic", &self.topic)
            .field("host", &self.host)
            .finish()
    }
}

impl ClientCommunicationHandler {
    fn new(
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
            host_conn: None,
            relay_conn: None,
            core_receiver,
            message_sender,
            video_status: VideoStatusMsg::default(),
            is_seeking: false,
            delay: Duration::default(),
        }
    }

    fn handle_broadcast(&mut self, peer_id: PeerId, msg: Vec<u8>) -> Result<()> {
        let niketsu_msg = msg.try_into()?;

        match niketsu_msg {
            NiketsuMessage::VideoStatus(video_status) => {
                if peer_id != self.host {
                    bail!("Received video status from non-host peer: {peer_id:?}")
                }
                return self.handle_video_status(video_status);
            }
            NiketsuMessage::Seek(_) => {
                self.is_seeking = true;
            }
            _ => {}
        }
        self.message_sender
            .send(niketsu_msg)
            .map_err(anyhow::Error::from)
    }

    fn handle_video_status(&mut self, mut msg: VideoStatusMsg) -> Result<()> {
        // TODO delete this
        if self.is_seeking {
            debug!("can not determine client position during seek");
            return Ok(());
        }

        if let Some(pos) = msg.position {
            debug!("add delay to position");
            if !msg.paused {
                msg.position = Some(pos + self.delay.div_f64(2.0));
            }
        }

        self.message_sender
            .send(msg.into())
            .map_err(anyhow::Error::from)
    }
}

#[async_trait]
impl CommunicationHandler for ClientCommunicationHandler {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                msg = self.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "Message from core");
                        if let Err(error) = self.handle_message(msg).await {
                            error!(%error, "Handling message caused error");
                        }
                    },
                    None => {
                        debug!("Channel of core closed. Stopping p2p client event loop");
                        break
                    }
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                result,
                peer,
                connection,
            })) => {
                debug!("Received ping!");
                if peer != self.host {
                    return;
                }

                if let Some(conn) = self.host_conn {
                    if connection == conn {
                        match result {
                            Ok(d) => self.delay = d,
                            Err(error) => warn!(%error, "Failed to get ping rtt"),
                        }
                    }
                };
            }
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(dcutr::Event {
                remote_peer_id,
                result,
            })) => match result {
                Ok(res) => {
                    self.host_conn = Some(res);

                    info!("Established direct connection. Closing connection to relay");
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .add_explicit_peer(&remote_peer_id);
                }
                Err(error) => {
                    error!(%remote_peer_id, %error, "Direct connection (hole punching) failed");
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => {
                debug!(%id, %peer_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                if let Err(error) = self.handle_broadcast(peer_id, message.data) {
                    error!(%error, "Failed to handle incoming message");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    if let Err(error) = self.message_sender.send(request.0) {
                        error!(%error, "Failed to send direct message to core");
                    }
                    if let Err(e) = self
                        .swarm
                        .send_response(channel, MessageResponse(ResponseStatus::Ok))
                    {
                        warn!(error = ?e)
                    }
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                }
            },
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => {
                if peer_id != self.host {
                    return;
                }

                if endpoint.is_relayed() {
                    self.relay_conn = Some(connection_id);
                }

                info!(%connection_id, ?endpoint, "Connection to host established!");
                if let Err(error) = self.message_sender.send(ConnectedMsg.into()) {
                    warn!(%error, "Failed to send connected message to core");
                }
            }
            SwarmEvent::IncomingConnection { local_addr, .. } => {
                info!(%local_addr, "Received connection")
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => {
                if peer_id == self.host && !self.swarm.is_connected(&peer_id) {
                    warn!(?cause, ?peer_id, host = %self.host, %connection_id, "Connection to host closed");
                    self.core_receiver.close();
                }
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id,
                error,
                connection_id,
            } => {
                let Some(pid) = peer_id else {
                    warn!(%error, "Outgoing connection error with unknown peer");
                    return;
                };

                if let Some(conn) = self.host_conn {
                    if connection_id != conn {
                        warn!(%error, %connection_id, "Outgoing connection error with non-host. Ignoring");
                        return;
                    }
                }

                if pid == self.host {
                    warn!(?error, ?peer_id, host = %self.host, %connection_id, "Connection error to host");
                    self.core_receiver.close();
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::OutboundFailure { peer, error, .. },
            )) => {
                if peer == self.host {
                    warn!("Outbound failure for request response with peer: error: {error:?} from {peer:?} where host {:?}", self.host);
                    // self.core_receiver.close();
                }
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => debug!(%peer_id, "Dialing event"),
            event => debug!(?event, "Received non-captured event"),
        }
    }

    async fn handle_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(?msg, host = %self.host, peer = %self.swarm.local_peer_id(), "Handling core message");

        match msg {
            NiketsuMessage::VideoStatus(vs) => {
                if vs.position != self.video_status.position {
                    self.is_seeking = false;
                    self.video_status = vs;
                }
                return Ok(());
            }
            NiketsuMessage::Playlist(_) => {
                // TODO can be moved to client, but client needs info on playlist + video selection
                self.swarm.send_request(&self.host, msg);
                return Ok(());
            }
            NiketsuMessage::Status(_) => {
                self.swarm.send_request(&self.host, msg);
                return Ok(());
            }
            _ => {}
        }
        self.swarm.try_broadcast(self.topic.clone(), msg)
    }
}

trait Sender<T, TResponse> {
    fn send_request(&mut self, peer_id: &PeerId, msg: T);
    fn send_response(&mut self, channel: ResponseChannel<TResponse>, msg: TResponse) -> Result<()>;
    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: T) -> Result<()>;
}

impl Sender<NiketsuMessage, MessageResponse> for Swarm<Behaviour> {
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
                PublishError::InsufficientPeers => {
                    debug!("Gossipsub insufficient peers. Publishing when no one is connected");
                    Ok(())
                }
                err => Err(anyhow::Error::from(err)),
            },
        }
    }
}

struct HostCommunicationHandler {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    relay: Multiaddr,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    status_list: UserStatusListMsg,
    playlist: PlaylistMsg,
    select: SelectMsg,
    users: HashMap<PeerId, UserStatus>,
}

impl fmt::Debug for HostCommunicationHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCommunicationHandler")
            .field("topic", &self.topic)
            .field("host", &self.host)
            .field("relay", &self.relay)
            .field("status_list", &self.status_list)
            .field("playlist", &self.playlist)
            .field("selected video", &self.select)
            .field("users", &self.users)
            .finish()
    }
}

impl HostCommunicationHandler {
    #[allow(clippy::too_many_arguments)]
    fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
        room: RoomName,
        playlist_handler: PlaylistHandler,
    ) -> Self {
        let playlist = PlaylistMsg {
            actor: arcstr::literal!("host"),
            playlist: playlist_handler.get_playlist(),
        };
        let select = SelectMsg {
            actor: arcstr::literal!("host"),
            position: Duration::default(),
            video: playlist_handler.get_current_video(),
        };
        message_sender.send(playlist.clone().into()).ok();
        message_sender.send(select.clone().into()).ok();
        Self {
            swarm,
            topic,
            host,
            relay,
            core_receiver,
            message_sender,
            status_list: UserStatusListMsg {
                room_name: room,
                users: BTreeSet::default(),
            },
            playlist,
            users: HashMap::default(),
            select,
        }
    }

    fn send_init_status(&mut self, peer_id: PeerId) -> Result<()> {
        self.swarm
            .send_request(&peer_id, NiketsuMessage::Playlist(self.playlist.clone()));

        self.swarm
            .send_request(&peer_id, NiketsuMessage::Select(self.select.clone()));

        self.swarm.try_broadcast(
            self.topic.clone(),
            NiketsuMessage::StatusList(self.status_list.clone()),
        )
    }

    fn update_status(&mut self, status: UserStatus, peer_id: PeerId) {
        self.status_list.users.replace(status.clone());
        self.users.insert(peer_id, status);
    }

    fn remove_peer(&mut self, status: &UserStatus, peer_id: &PeerId) {
        self.status_list.users.remove(status);
        self.users.remove(peer_id);
    }

    //TODO: consider caching?
    fn all_users_ready(&self) -> bool {
        self.status_list.users.iter().all(|u| u.ready)
    }

    fn roll_new_username(&self, username: ArcStr) -> ArcStr {
        //TODO only needs to be applied once?
        // check same user
        let mut buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        while self.username_exists(buzzword.clone()) {
            buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        }
        buzzword
    }

    fn is_established_user(&self, peer_id: PeerId) -> bool {
        self.users.contains_key(&peer_id)
    }

    fn username_exists(&self, name: ArcStr) -> bool {
        self.status_list.users.iter().any(|u| u.name == name)
    }

    fn handle_status(&mut self, status: UserStatus, peer_id: PeerId) {
        let mut new_status = status.clone();

        if self.is_established_user(peer_id) {
            if !self.username_exists(status.name.clone()) {
                // user name change, remove old status and update user map
                let users = self.users.clone(); // is there any other way to avoid mut/immut borrow?
                let old_status = users.get(&peer_id).expect("User should exist");
                self.remove_peer(old_status, &peer_id);
            }
            // otherwise, typical user status update, only need to update map & list
        } else {
            // new user
            if self.username_exists(status.name.clone()) {
                // username needs to be force changed to avoid duplicate user names
                let new_username = self.roll_new_username(status.name.clone());
                new_status = UserStatus {
                    name: new_username,
                    ready: status.ready,
                };
                self.swarm
                    .send_request(&peer_id, NiketsuMessage::Status(new_status.clone()));
            }
        }

        self.update_status(new_status, peer_id);
    }

    fn handle_all_users_ready(&mut self, peer_id: &PeerId) -> Result<()> {
        debug!(ready=%self.all_users_ready(),"all users ready");
        if self.all_users_ready() {
            debug!("All users area ready. Publishing start to gossipsub");
            let mut start_msg = NiketsuMessage::Start(StartMsg {
                actor: arcstr::literal!(""),
            });
            if let Some(user) = self.users.get(peer_id) {
                start_msg = NiketsuMessage::Start(StartMsg {
                    actor: user.name.clone(),
                });
            }
            self.message_sender.send(start_msg.clone())?;
            self.swarm.try_broadcast(self.topic.clone(), start_msg)?;
        }
        Ok(())
    }

    fn select_next(&mut self, new_playlist: &PlaylistMsg) -> Option<SelectMsg> {
        if new_playlist.playlist.len() >= self.playlist.playlist.len() {
            return None;
        }

        let mut new_position = 0;
        let max_len = new_playlist.playlist.len();
        if let Some(current_video) = self.select.video.clone() {
            for old_video in self.playlist.playlist.iter() {
                if let Some(new_video) = new_playlist.playlist.get(new_position) {
                    debug!(?current_video, ?new_video, "Current video and old");
                    if *new_video == current_video {
                        // No need to select if current video is still in playlist
                        return None;
                    }

                    if *old_video == current_video {
                        break;
                    }

                    if *new_video == *old_video {
                        new_position += 1;
                    }

                    if new_position >= max_len {
                        new_position -= 1;
                        break;
                    }
                }
            }
        }

        new_playlist
            .playlist
            .get(new_position)
            .map(|new_select| SelectMsg {
                actor: arcstr::format!("host"),
                position: Duration::ZERO,
                video: Some(new_select.clone()),
            })
    }

    fn handle_new_playlist(&mut self, playlist: &PlaylistMsg, peer_id: PeerId) -> Result<()> {
        if let Some(select_msg) = self.select_next(&playlist) {
            self.select = select_msg.clone();
            let msg: NiketsuMessage = select_msg.into();
            self.message_sender.send(msg.clone())?;
            self.swarm.try_broadcast(self.topic.clone(), msg)?;
            self.handle_all_users_ready(&peer_id)?;
        }
        Ok(())
    }

    fn handle_incoming_message(&mut self, peer_id: PeerId, mut msg: NiketsuMessage) -> Result<()> {
        match msg.clone() {
            NiketsuMessage::Status(status) => {
                self.handle_status(status.clone(), peer_id);
                self.handle_all_users_ready(&peer_id)?;
                msg = NiketsuMessage::StatusList(self.status_list.clone());
                self.message_sender.send(msg.clone())?;
                self.swarm.try_broadcast(self.topic.clone(), msg)?;
            }
            NiketsuMessage::Playlist(playlist) => {
                self.message_sender.send(msg.clone())?;
                self.swarm.try_broadcast(self.topic.clone(), msg)?;
                self.handle_new_playlist(&playlist, peer_id)?;
                self.playlist = playlist;
            }
            _ => {
                bail!("Host received unexpected direct message: {msg:?}");
            }
        }
        Ok(())
    }

    fn handle_broadcast(&mut self, msg: Vec<u8>, peer_id: &PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        match niketsu_msg.clone() {
            NiketsuMessage::Select(select) => {
                self.handle_all_users_ready(peer_id)?;
                self.select = select;
            }
            NiketsuMessage::Join(_)
            | NiketsuMessage::VideoStatus(_)
            | NiketsuMessage::StatusList(_)
            | NiketsuMessage::ServerMessage(_)
            | NiketsuMessage::Status(_)
            | NiketsuMessage::Playlist(_)
            | NiketsuMessage::Connection(_) => {
                bail!("Host received unexpected broadcast message: {niketsu_msg:?}")
            }
            _ => {}
        }
        self.message_sender.send(niketsu_msg)?;
        Ok(())
    }
}

#[async_trait]
impl CommunicationHandler for HostCommunicationHandler {
    async fn run(&mut self) {
        if let Err(error) = self.message_sender.send(ConnectedMsg.into()) {
            warn!(%error, "Failed to send connected message to core");
        }

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                msg = self.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "core message");
                        if let Err(error) = self.handle_message(msg).await {
                            error!(%error, "Handling message caused error");
                        }
                    },
                    None => {
                        debug!("Channel of core closed. Stopping p2p client event loop");
                        break
                    },
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(dcutr::Event {
                remote_peer_id,
                result,
            })) => {
                debug!(?result, ?remote_peer_id, "dcutr result");
            }
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => {
                debug!(%id, %peer_id, msg = %String::from_utf8_lossy(&message.data), "Got message");
                if let Err(error) = self.handle_broadcast(message.data, &peer_id) {
                    error!(%error, "Failed to handle incoming broadcast message");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { peer, message, .. },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    if let Err(error) = self.handle_incoming_message(peer, request.0) {
                        error!(%error, "Failed to handle incoming message");
                    }
                    if let Err(e) = self
                        .swarm
                        .send_response(channel, MessageResponse(ResponseStatus::Ok))
                    {
                        warn!(error = ?e)
                    }
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                }
            },
            SwarmEvent::IncomingConnection { local_addr, .. } => {
                debug!(%local_addr, "Received connection")
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                debug!(%peer_id, "New client established connection");
                if let Err(error) = self.send_init_status(peer_id) {
                    error!(%error, "Failed to send initial messages to client");
                }
            }
            SwarmEvent::ConnectionClosed {
                peer_id, endpoint, ..
            } => {
                if self.relay == (*endpoint.get_remote_address()) {
                    error!(?endpoint, "Connection of host to relay server closed");
                    self.core_receiver.close();
                } else if !self.swarm.is_connected(&peer_id) {
                    debug!("User connection stopped and user removed from map");
                    let users = self.users.clone();
                    if let Some(status) = users.get(&peer_id) {
                        self.remove_peer(status, &peer_id);
                        let status_list = NiketsuMessage::StatusList(self.status_list.clone());
                        if let Err(error) = self.message_sender.send(status_list.clone()) {
                            error!(%error, "Failed to send status list to core");
                        }
                        if let Err(error) =
                            self.swarm.try_broadcast(self.topic.clone(), status_list)
                        {
                            error!(%error, "Failed to broadcast status list");
                        }
                    } else {
                        warn!("Expected peer to be included in list");
                    }
                }
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => debug!(%peer_id, "Dialing"),
            event => debug!(?event, "Received non-captured event"),
        }
    }

    async fn handle_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        let mut niketsu_msg = msg.clone();
        debug!(host = %self.host, ?msg, "Handling core message");

        match niketsu_msg.clone() {
            NiketsuMessage::Status(status) => {
                debug!(?status);
                let peer_id = self.host;
                self.update_status(status, peer_id);
                self.handle_all_users_ready(&peer_id)?;

                niketsu_msg = NiketsuMessage::StatusList(self.status_list.clone());
                debug!(status_list = ?niketsu_msg);
                self.message_sender.send(niketsu_msg.clone())?;
            }
            NiketsuMessage::Playlist(playlist) => {
                self.handle_new_playlist(&playlist, self.host)?;
                self.playlist = playlist;
            }
            NiketsuMessage::VideoStatus(status) => {
                self.select.position = status.position.unwrap_or_default();
            }
            NiketsuMessage::Select(select) => {
                debug!(?select);
                self.select = select;
                self.swarm.try_broadcast(self.topic.clone(), niketsu_msg)?;
                let host = self.host;
                self.handle_all_users_ready(&host)?;
                return Ok(());
            }
            _ => {}
        }

        self.swarm.try_broadcast(self.topic.clone(), niketsu_msg)?;
        Ok(())
    }
}
