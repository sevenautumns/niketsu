use anyhow::{bail, Context, Result};
use futures::{FutureExt, StreamExt};
use libp2p::identity;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::SwarmEvent;
use libp2p::{
    dcutr, gossipsub, identify, kad, noise, ping, relay, swarm::NetworkBehaviour, tcp, yamux,
    Multiaddr, PeerId,
};
use log::info;
use std::collections::HashSet;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::time::Duration;
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::spawn;
use uuid::Uuid;

use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::Swarm;

use crate::messages::NiketsuMessage;
use bcrypt::{hash, verify, DEFAULT_COST};
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, HashMap};

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

    fn verify(&self, password: String) -> bool {
        match verify(password, &self.password) {
            Err(e) => return false,
            Ok(valid) => return valid,
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: u8,              // 0 -> ok, 1 -> err
    peer_id: Option<PeerId>, // peer id of room if found
}

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

        info!("Listening on relay");
        self.listen_on(relay_addr.clone().with(Protocol::P2pCircuit))
            .expect("Failed to listen on remote relay");

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
    receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    relay_addr: Multiaddr,
    event_loop: Option<EventLoop>,
}

impl P2PClient {
    pub(crate) async fn new(room: String, password: String, secure: bool) -> Result<P2PClient> {
        let key_pair = identity::Keypair::generate_ed25519();
        let peer_id = key_pair.public().to_peer_id();

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
            .establish_conection(relay_addr.clone(), room, password)
            .await
        {
            Err(e) => panic!("{e:?}"),
            Ok(h) => host = h,
        }

        let topic = gossipsub::IdentTopic::new("test-net");
        swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

        let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (event_sender, event_receiver) = tokio::sync::mpsc::unbounded_channel();
        let client = P2PClient {
            sender: command_sender,
            receiver: event_receiver,
            relay_addr,
            event_loop: Some(EventLoop::new(
                swarm,
                topic,
                host,
                command_receiver,
                event_sender,
            )),
        };

        Ok(client)
    }

    pub(crate) fn run(&mut self) -> tokio::task::JoinHandle<()> {
        let eventloop = self.event_loop.take().unwrap();
        spawn(eventloop.run())
    }

    pub(crate) async fn next(&mut self) -> Option<Vec<u8>> {
        self.receiver.recv().await
    }

    pub(crate) fn send(&self, msg: NiketsuMessage) -> Result<()> {
        Ok(self.sender.send(msg)?)
    }
}

pub(crate) struct EventLoop {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: Host,
    message_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    connected_clients: HashMap<String, PeerId>,
    connected_peers: HashMap<PeerId, String>,
}

impl fmt::Debug for EventLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventLoop")
            .field("topic", &self.topic)
            .field("host", &self.host)
            .field("connected_clients", &self.connected_clients)
            .field("connected_peers", &self.connected_peers)
            .finish()
    }
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: Host,
        command_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        event_sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    ) -> Self {
        Self {
            swarm,
            topic,
            host,
            message_receiver: command_receiver,
            message_sender: event_sender,
            connected_clients: Default::default(),
            connected_peers: Default::default(),
        }
    }

    pub(crate) async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                command = self.message_receiver.recv() => match command {
                    Some(c) => {
                        info!("{c:?}");
                        self.handle_message(c).await
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
                info!(
                    "Got message: '{}'\n with id: {id} from peer: {peer_id}",
                    String::from_utf8_lossy(&message.data),
                );
                //TODO handle some messages already ...
                self.message_sender.send(message.data);
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request { request, .. } => {
                    self.message_sender.send(request.0);
                }
                request_response::Message::Response { .. } => todo!(),
            },
            // SwarmEvent::IncomingConnection { .. } => {}
            // SwarmEvent::ConnectionClosed { .. } => {}
            // SwarmEvent::OutgoingConnectionError { .. } => {}
            // SwarmEvent::IncomingConnectionError { .. } => {}
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => info!("Dialing {peer_id}"),
            e => info!("{e:?}"),
        }
    }

    async fn handle_message(&mut self, msg: NiketsuMessage) {
        //TODO some message need further preparations
        //TODO maybe need to add peer addr manually
        //TODO what about broadcasting as host?
        let req = serde_json::to_string(&msg).unwrap();
        info!(
            "host: {:?}, peer: {:?}",
            self.host,
            self.swarm.local_peer_id()
        );
        if self.host != (*self.swarm.local_peer_id()) {
            info!("Sending message to host: {req:?}");
            self.swarm
                .behaviour_mut()
                .message_request_response
                .send_request(
                    &self.host,
                    MessageRequest {
                        0: req.into_bytes(),
                    },
                );
        } else {
            info!("Publishing message to gossipsub: {req:?}");
            self.swarm
                .behaviour_mut()
                .gossipsub
                .publish(self.topic.clone(), req.into_bytes());
        }
    }
}
