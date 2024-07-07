use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bcrypt::{hash, DEFAULT_COST};
use enum_dispatch::enum_dispatch;
use futures::StreamExt;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    dcutr, gossipsub, identify, identity, kad, noise, ping, relay, tcp, yamux, Multiaddr, PeerId,
    StreamProtocol, SwarmBuilder,
};
use log::{debug, error, info, warn};
use niketsu_core::user::UserStatus;
use serde::{Deserialize, Serialize};
use tokio::{io, spawn};
use uuid::Uuid;

use crate::messages::{NiketsuMessage, StatusListMessage, UserStatusMessage, VideoStatusMessage};

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

//TODO: Can probably be changed to NiketsuMessage
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MessageRequest(Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageResponse(Vec<u8>);

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
        }
        self.listen_on(relay_addr.clone().with(Protocol::P2pCircuit))
            .expect("Failed to listen on remote relay");

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
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
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
    fn handle_incoming_message(&mut self, peer_id: PeerId, msg: Vec<u8>) -> Result<()> {
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
                if let Err(e) = self.handle_incoming_message(peer_id, message.data) {
                    error!("Failed to handle incoming message: {e:?}");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request { request, .. } => {
                    info!("Received direct message {:?}", request.clone());
                    // let err = self.message_sender.send(request.0);
                }
                request_response::Message::Response { .. } => info!("Received direct response"),
            },
            SwarmEvent::IncomingConnection { local_addr, .. } => {
                info!("Received connection from {local_addr:?}")
            }
            SwarmEvent::ConnectionClosed {
                peer_id, endpoint, ..
            } => {
                if self.relay == (*endpoint.get_remote_address()) || peer_id == self.host {
                    warn!("Connection of client to relay server or host closed: {endpoint:?}");
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
                    .send_request(&self.host, MessageRequest { 0: req });
            }
        }
        Ok(())
    }
}

struct HostCommunicationHandler {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    relay: Multiaddr,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    room: String,
    status_list: BTreeMap<String, BTreeSet<UserStatusMessage>>,
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
        relay: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
        room: String,
    ) -> Self {
        let mut map: BTreeMap<String, BTreeSet<UserStatusMessage>> = BTreeMap::default();
        map.insert(room.clone(), BTreeSet::default());

        Self {
            swarm,
            topic,
            relay,
            core_receiver,
            message_sender,
            room,
            status_list: map,
            users: HashMap::default(),
        }
    }

    fn update_status_list(&mut self, status: UserStatusMessage) {
        self.status_list.entry(self.room.clone()).and_modify(|set| {
            set.insert(status);
        });
    }

    fn get_status_list(&mut self) -> NiketsuMessage {
        NiketsuMessage::StatusList(StatusListMessage {
            rooms: self.status_list.clone(),
        })
    }

    fn handle_incoming_message(&mut self, peer_id: PeerId, msg: Vec<u8>) -> Result<()> {
        //TODO if all are ready, cache full, etc. then start
        //TODO if client sends username that already exists, f* him
        let mut niketsu_msg: NiketsuMessage = msg.clone().try_into()?;
        info!("Received message {:?}", niketsu_msg.clone());
        if let NiketsuMessage::Status(status) = niketsu_msg.clone() {
            self.users.insert(peer_id, status.clone());
            //TODO delete old username in case it exists
            self.update_status_list(status);
            niketsu_msg = self.get_status_list();
        }
        self.message_sender.send(niketsu_msg)?;
        debug!("Publishing message from peer {peer_id:?} to gossipsub");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), msg)?;

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
            SwarmEvent::ConnectionClosed {
                peer_id, endpoint, ..
            } => {
                if self.relay == (*endpoint.get_remote_address()) {
                    warn!("Connection of client to relay server or host closed: {endpoint:?}");
                    self.core_receiver.close();
                    //TODO panic
                } else {
                    info!("User connection stopped and user removed from map");
                    self.users.remove(&peer_id);
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
        //TODO maybe need to add peer addr manually
        //TODO what about broadcasting as host?
        let mut req: Vec<u8> = msg.clone().try_into()?;
        info!("host: {:?}", self.swarm.local_peer_id());
        if let NiketsuMessage::Status(status) = msg.clone() {
            //TODO What is the behavior, how to update user status/ready status?
            self.update_status_list(status);
            let status_list = self.get_status_list();
            self.message_sender.send(status_list.clone())?;
            req = status_list.try_into()?;
        }
        debug!("Publishing message to gossipsub: {msg:?}");
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), req)?;
        Ok(())
    }
}
