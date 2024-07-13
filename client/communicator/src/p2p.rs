use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bcrypt::{hash, DEFAULT_COST};
use enum_dispatch::enum_dispatch;
use fake::faker::company::en::Buzzword;
use fake::Fake;
use futures::StreamExt;
use libp2p::gossipsub::PublishError;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    dcutr, gossipsub, identify, identity, kad, noise, ping, relay, tcp, yamux, Multiaddr, PeerId,
    StreamProtocol,
};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use tokio::{io, spawn};
use uuid::Uuid;

use crate::messages::{
    NiketsuMessage, StartMessage, StatusListMessage, UserStatusMessage, VideoStatusMessage,
};

//TODO: proper peer id for relay
// 89.58.15.23
const RELAY_ADDRESS: &str =
    "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    message_request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

impl Deref for Behaviour {
    type Target = kad::Behaviour<MemoryStore>;

    fn deref(&self) -> &Self::Target {
        &self.kademlia
    }
}

impl DerefMut for Behaviour {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kademlia
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MessageRequest(NiketsuMessage);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageResponse(NiketsuMessage);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    // should be hashed when listening
    room: String,
    password: String,
}

impl InitRequest {
    fn new(room: String, password: String) -> Self {
        Self {
            room,
            password: hash(password, DEFAULT_COST).expect("Failed to hash password"),
        }
    }

    fn new_without_hash(room: String, password: String) -> Self {
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
        room: String,
        password: String,
    ) -> Result<Host>;
    async fn identify_loop(&mut self, room: String, password: String) -> Option<PeerId>;
    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
    ) -> Result<Option<PeerId>>;
}

type Host = PeerId;

#[async_trait]
impl SwarmRelayConnection for Swarm<Behaviour> {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
    ) -> Result<Host> {
        self.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        self.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        let host_peer_id = self
            .identify_relay(relay_addr.clone(), room, password)
            .await;

        info!("Peer id from relay: {host_peer_id:?}");
        if let Some(peer_id) = host_peer_id? {
            info!("Dialing peer: {:?}", peer_id);
            self.dial(
                relay_addr
                    .clone()
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(peer_id)),
            )
            .unwrap();
            self.behaviour_mut()
                .add_address(&peer_id, "/dnsaddr/bootstrap.libp2p.io".parse()?);
            return Ok(peer_id);
        } else {
            info!("Listening on relay");
            self.listen_on(relay_addr.clone().with(Protocol::P2pCircuit))
                .expect("Failed to listen on remote relay");
        }

        Ok(*self.local_peer_id())
    }

    async fn identify_loop(&mut self, room: String, password: String) -> Option<PeerId> {
        let mut host_peer_id: Option<PeerId> = None;
        let mut learned_observed_addr = false;
        let mut told_relay_observed_addr = false;
        let mut observed_peer = false;

        loop {
            match self.next().await.unwrap() {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Sent {
                    ..
                })) => {
                    info!("Told relay its public address");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    peer_id,
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    info!("Relay told us our observed address: {observed_addr}");
                    info!("Sending new room request to relay");
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
                            observed_peer = true;
                        }
                    }
                }
                event => info!("Received other relay events: {event:?}"),
            }

            if learned_observed_addr && told_relay_observed_addr && observed_peer {
                break;
            }
        }

        host_peer_id
    }

    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
    ) -> Result<Option<PeerId>> {
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
    relay_addr: Multiaddr,
    handler: tokio::task::JoinHandle<()>,
}

impl P2PClient {
    pub(crate) async fn new(room: String, password: String, secure: bool) -> Result<P2PClient> {
        let key_pair = identity::Keypair::generate_ed25519();

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(key_pair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().port_reuse(true).nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|keypair, relay_behaviour| {
                let message_id_fn = |_message: &gossipsub::Message| {
                    let id = Uuid::new_v4();
                    gossipsub::MessageId::from(id)
                };

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .duplicate_cache_time(Duration::from_secs(1))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .message_id_fn(message_id_fn)
                    .build()
                    .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?;

                let mut cfg = kad::Config::default();
                cfg.set_query_timeout(Duration::from_secs(5 * 60));

                Ok(Behaviour {
                    relay_client: relay_behaviour,
                    ping: ping::Behaviour::new(ping::Config::new()),
                    identify: identify::Behaviour::new(identify::Config::new(
                        "/identify/1".to_string(),
                        keypair.public(),
                    )),
                    dcutr: dcutr::Behaviour::new(keypair.public().to_peer_id()),
                    gossipsub: gossipsub::Behaviour::new(
                        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
                        gossipsub_config,
                    )?,
                    kademlia: kad::Behaviour::with_config(
                        keypair.public().to_peer_id(),
                        kad::store::MemoryStore::new(keypair.public().to_peer_id()),
                        cfg,
                    ),
                    message_request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/niketsu-message/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default(),
                    ),
                    init_request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/authorisation/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default(),
                    ),
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
            .build();

        let relay_addr =
            Multiaddr::from_str(RELAY_ADDRESS).expect("Relay address could not be parsed");

        let host: Host;
        match swarm
            .establish_conection(relay_addr.clone(), room.clone(), password.clone())
            .await
        {
            Err(e) => panic!("{e:?}"),
            Ok(h) => host = h,
        }

        let topic = gossipsub::IdentTopic::new(format!("{room}|{password}"));
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
            ))
        } else {
            Handler::Client(ClientCommunicationHandler {
                swarm,
                topic,
                host,
                relay: relay_addr.clone(),
                core_receiver,
                message_sender,
            })
        };

        let client = P2PClient {
            sender: core_sender,
            receiver: message_receiver,
            relay_addr,
            handler: spawn(async move { handler.run().await }),
        };

        Ok(client)
    }

    pub(crate) async fn next(&mut self) -> Option<NiketsuMessage> {
        self.receiver.recv().await
    }

    pub(crate) fn send(&self, msg: NiketsuMessage) -> Result<()> {
        info!("Sending message {msg:?}");
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
    relay: Multiaddr,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
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
    fn handle_broadcast(&mut self, peer_id: PeerId, msg: Vec<u8>) -> Result<()> {
        let niketsu_msg = msg.try_into()?;
        if let NiketsuMessage::UserMessage(_) = niketsu_msg {
            return self
                .message_sender
                .send(niketsu_msg)
                .map_err(anyhow::Error::from);
        }

        if peer_id != self.host {
            bail!("Received broadcast from non-host peer: {peer_id:?}")
        }

        if let NiketsuMessage::VideoStatus(video_status) = niketsu_msg {
            return self
                .handle_video_status(video_status)
                .map_err(anyhow::Error::from);
        }

        self.message_sender
            .send(niketsu_msg)
            .map_err(anyhow::Error::from)
    }

    fn handle_video_status(&mut self, msg: VideoStatusMessage) -> Result<()> {
        info!("Not yet implemented!");
        Ok(())
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
                        info!("{msg:?}");
                        if let Err(e) = self.handle_message(msg).await {
                            error!("Handling message caused error {e:?}");
                        }
                    },
                    None => continue, // channel closed?
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => {
                debug!(
                    "Received gossipsub message: '{}'\n with id: {id} from peer: {peer_id}",
                    String::from_utf8_lossy(&message.data),
                );
                if let Err(e) = self.handle_broadcast(peer_id, message.data) {
                    error!("Failed to handle incoming message: {e:?}");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request { request, .. } => {
                    info!("Received direct message {:?}", request.clone());
                    if let Err(e) = self.message_sender.send(request.0) {
                        error!("Failed to send direct message to core: {e:?}");
                    }
                }
                request_response::Message::Response { .. } => info!("Received direct response"),
            },
            SwarmEvent::IncomingConnection { local_addr, .. } => {
                info!("Received connection from {local_addr:?}")
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                ..
            } => {
                // client losing connection to relay might be fine
                if peer_id == self.host {
                    warn!("Connection of client to relay server or host closed: {endpoint:?} with cause: {cause:?}");
                    self.core_receiver.close();
                    //TODO panic
                }
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => info!("Dialing {peer_id}"),
            e => debug!("Received non-captured event: {e:?}"),
        }
    }

    async fn handle_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        //TODO some message need further preparations
        // maybe need to add peer addr manually
        // what about broadcasting as host?
        debug!(
            "Handling core message {msg:?} for host {} of peer {}",
            self.host,
            self.swarm.local_peer_id()
        );

        let req: Vec<u8> = msg.clone().try_into()?;
        match msg {
            NiketsuMessage::UserMessage(_) => {
                debug!("Publishing message to gossipsub");
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.topic.clone(), req)?;
            }
            NiketsuMessage::VideoStatus(vs) => {
                debug!("Dropping video status of client {vs:?}");
            }
            _ => {
                debug!("Sending message to host");
                self.swarm
                    .behaviour_mut()
                    .message_request_response
                    .send_request(&self.host, MessageRequest { 0: msg });
            }
        }
        Ok(())
    }
}

trait Sender<T> {
    fn send(&mut self, peer_id: &PeerId, msg: T);
    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: T) -> Result<()>;
}

impl Sender<NiketsuMessage> for Swarm<Behaviour> {
    fn send(&mut self, peer_id: &PeerId, msg: NiketsuMessage) {
        // ignores outbound id
        self.behaviour_mut()
            .message_request_response
            .send_request(peer_id, MessageRequest { 0: msg });
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
                err => return Err(anyhow::Error::from(err)),
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
    status_list: StatusListMessage,
    users: HashMap<PeerId, UserStatusMessage>,
}

impl fmt::Debug for HostCommunicationHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCommunicationHandler")
            .field("topic", &self.topic)
            .finish()
    }
}

impl HostCommunicationHandler {
    fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
        room: String,
    ) -> Self {
        Self {
            swarm,
            topic,
            host,
            relay,
            core_receiver,
            message_sender,
            status_list: StatusListMessage {
                room_name: room,
                users: BTreeSet::default(),
            },
            users: HashMap::default(),
        }
    }

    fn send_init_status(&mut self, peer_id: PeerId) {
        //TODO maybe unnecessary
        if let Err(e) = self.swarm.try_broadcast(
            self.topic.clone(),
            NiketsuMessage::StatusList(self.status_list.clone()),
        ) {
            error!("Failed to broadcast status list: {e:?}");
        }

        //TODO send playlist and check username?
    }

    fn update_status(&mut self, status: UserStatusMessage, peer_id: PeerId) {
        self.status_list.users.replace(status.clone());
        self.users.insert(peer_id, status);
    }

    fn remove_peer(&mut self, status: &UserStatusMessage, peer_id: &PeerId) {
        self.status_list.users.remove(status);
        self.users.remove(peer_id);
    }

    //TODO caching?
    fn all_users_ready(&self) -> bool {
        self.status_list.users.iter().all(|u| u.ready)
    }

    fn roll_new_username(&self, username: String) -> String {
        //TODO only needs to be applied once?
        // check same user
        let mut buzzword: String = format!("{username}_{}", Buzzword().fake::<String>());
        while self.username_exists(buzzword.clone()) {
            buzzword = format!("{username}_{}", Buzzword().fake::<String>());
        }
        buzzword
    }

    fn is_established_user(&self, peer_id: PeerId) -> bool {
        self.users.get(&peer_id).is_some()
    }

    fn username_exists(&self, username: String) -> bool {
        self.status_list
            .users
            .iter()
            .any(|u| u.username == username)
    }

    fn handle_status(&mut self, status: UserStatusMessage, peer_id: PeerId) {
        let mut new_status = status.clone();

        if self.is_established_user(peer_id) {
            if !self.username_exists(status.username.clone()) {
                // user name change, remove old status and update user map
                let users = self.users.clone(); // is there any other way to avoid mut/immut borrow?
                let old_status = users.get(&peer_id).expect("User should exist");
                self.remove_peer(&old_status, &peer_id);
            }
            // otherwise, typical user status update, only need to update map & list
        } else {
            // new user
            if self.username_exists(status.username.clone()) {
                // username needs to be force changed to avoid duplicate user names
                let new_username = self.roll_new_username(status.username.clone());
                new_status = UserStatusMessage {
                    username: new_username,
                    ready: status.ready,
                };
                self.swarm
                    .send(&peer_id, NiketsuMessage::Status(new_status.clone()));
            }
        }

        self.update_status(new_status, peer_id);
    }

    fn handle_all_users_ready(&mut self, peer_id: &PeerId) -> Result<()> {
        if self.all_users_ready() {
            debug!("All users area ready. Publishing start to gossipsub");
            let mut start_msg = NiketsuMessage::Start(StartMessage {
                username: "".to_string(),
            });
            if let Some(user) = self.users.get(&peer_id) {
                start_msg = NiketsuMessage::Start(StartMessage {
                    username: user.username.clone(),
                });
            }
            self.message_sender.send(start_msg.clone())?;
            self.swarm.try_broadcast(self.topic.clone(), start_msg)?;
        }
        Ok(())
    }

    fn handle_incoming_message(&mut self, peer_id: PeerId, msg: NiketsuMessage) -> Result<()> {
        let mut niketsu_msg = msg;
        info!("Received message {:?}", niketsu_msg.clone());
        if let NiketsuMessage::Status(status) = niketsu_msg.clone() {
            self.handle_status(status.clone(), peer_id);
            niketsu_msg = NiketsuMessage::StatusList(self.status_list.clone());
        }
        self.handle_all_users_ready(&peer_id)?;

        self.message_sender.send(niketsu_msg.clone())?;
        debug!("Publishing message from peer {peer_id:?} to gossipsub");
        self.swarm.try_broadcast(self.topic.clone(), niketsu_msg)?;
        Ok(())
    }

    fn handle_broadcast(&mut self, peer_id: PeerId, msg: Vec<u8>) -> Result<()> {
        let niketsu_msg = msg.try_into()?;
        self.message_sender.send(niketsu_msg)?;
        Ok(())
    }
}

#[async_trait]
impl CommunicationHandler for HostCommunicationHandler {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                msg = self.core_receiver.recv() => match msg {
                    Some(msg) => {
                        info!("core message: {msg:?}");
                        if let Err(e) = self.handle_message(msg).await {
                            error!("Handling message caused error {e:?}");
                        }
                    },
                    None => {
                        debug!("Channel of core closed. Stopping p2p client event loop");
                        continue
                    },
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => {
                debug!(
                    "Got message: '{}'\n with id: {id} from peer: {peer_id}",
                    String::from_utf8_lossy(&message.data),
                );
                if let Err(e) = self.handle_broadcast(peer_id, message.data) {
                    error!("Failed to handle incoming message: {e:?}");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { peer, message, .. },
            )) => match message {
                request_response::Message::Request { request, .. } => {
                    if let Err(e) = self.handle_incoming_message(peer, request.0) {
                        error!("Failed to handle incoming message: {e:?}");
                    }
                }
                request_response::Message::Response { .. } => info!("Received direct response"),
            },
            SwarmEvent::IncomingConnection { local_addr, .. } => {
                info!("Received connection from {local_addr:?}")
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("New client established connection {peer_id}");
                self.send_init_status(peer_id);
            }
            SwarmEvent::ConnectionClosed {
                peer_id, endpoint, ..
            } => {
                if self.relay == (*endpoint.get_remote_address()) {
                    warn!("Connection of client to relay server or host closed: {endpoint:?}");
                    self.core_receiver.close();
                    //TODO panic
                } else {
                    info!("User connection stopped and user removed from map");
                    let users = self.users.clone();
                    let status = users
                        .get(&peer_id)
                        .expect("Connected peer should be included in lists");
                    self.remove_peer(&status, &peer_id);
                    let status_list = NiketsuMessage::StatusList(self.status_list.clone());
                    if let Err(e) = self.message_sender.send(status_list.clone()) {
                        error!("Failed to send status list to core: {e:?}");
                    }
                    if let Err(e) = self.swarm.try_broadcast(self.topic.clone(), status_list) {
                        error!("Failed to broadcast status list: {e:?}");
                    }
                }
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => info!("Dialing {peer_id}"),
            e => debug!("Received non-captured event: {e:?}"),
        }
    }

    async fn handle_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        let mut niketsu_msg = msg.clone();
        info!("host: {:?}", self.host);
        if let NiketsuMessage::Status(status) = niketsu_msg.clone() {
            //TODO What is the behavior, how to update user status/ready status?
            info!("Status {:?}", status.clone());
            let peer_id = self.host.clone();
            self.update_status(status, peer_id);
            self.handle_all_users_ready(&peer_id)?;

            niketsu_msg = NiketsuMessage::StatusList(self.status_list.clone());
            info!("Status list: {:?}", niketsu_msg.clone());
            self.message_sender.send(niketsu_msg.clone())?;
        }
        //TODO how to handle playlist?
        debug!("Publishing message to gossipsub: {msg:?}");
        self.swarm.try_broadcast(self.topic.clone(), niketsu_msg)?;
        Ok(())
    }
}
