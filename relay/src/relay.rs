use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_std::stream::StreamExt;
use async_std::sync::RwLock;
use bcrypt::{hash, verify, DEFAULT_COST};
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Multiaddr;
use libp2p::identity::Keypair;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identify, noise, ping, relay, tcp, yamux, PeerId, StreamProtocol, Swarm};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::config::Config;

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    room: String,
    password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ResponseStatus {
    Ok,
    Err,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: ResponseStatus,
    peer_id: Option<PeerId>, // peer id of room if found
}

struct PasswordHash(String);

impl From<InitRequest> for PasswordHash {
    fn from(value: InitRequest) -> Self {
        PasswordHash(hash(value.password.clone(), DEFAULT_COST).unwrap_or(value.password))
    }
}

impl PasswordHash {
    fn verify(&self, password: String) -> bool {
        verify(password, &self.0).unwrap_or(false)
    }
}

type RoomName = String;

pub struct Relay {
    swarm: Swarm<Behaviour>,
    rooms: Arc<RwLock<HashMap<RoomName, (PeerId, PasswordHash)>>>,
    hosts: Arc<RwLock<HashMap<PeerId, RoomName>>>,
}

pub fn new(config: Config) -> Result<Relay> {
    let keypair = Keypair::from_protobuf_encoding(config.keypair.unwrap().as_slice())?;
    let mut quic_config = libp2p::quic::Config::new(&keypair.clone());
    quic_config.handshake_timeout = Duration::from_secs(10);
    quic_config.max_idle_timeout = 5 * 1000;

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic_config(|_| quic_config)
        .with_behaviour(|key| Behaviour {
            relay: relay::Behaviour::new(
                key.public().to_peer_id(),
                libp2p::relay::Config::default(),
            ),
            ping: ping::Behaviour::new(
                ping::Config::new()
                    .with_timeout(Duration::from_secs(5))
                    .with_interval(Duration::from_secs(2)),
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/identify/1".to_string(),
                key.public(),
            )),
            init_request_response: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/authorisation/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    let listen_addr_tcp_ipv4 = Multiaddr::empty()
        .with(Protocol::from(Ipv4Addr::UNSPECIFIED))
        .with(Protocol::Tcp(config.port));
    let listen_addr_tcp_ipv6 = Multiaddr::empty()
        .with(Protocol::from(Ipv6Addr::UNSPECIFIED))
        .with(Protocol::Tcp(config.port));
    swarm.listen_on(listen_addr_tcp_ipv4)?;
    swarm.listen_on(listen_addr_tcp_ipv6)?;

    let listen_addr_quic_ipv4 = Multiaddr::empty()
        .with(Protocol::from(Ipv4Addr::UNSPECIFIED))
        .with(Protocol::Udp(config.port))
        .with(Protocol::QuicV1);
    let listen_addr_quic_ipv6 = Multiaddr::empty()
        .with(Protocol::from(Ipv6Addr::UNSPECIFIED))
        .with(Protocol::Udp(config.port))
        .with(Protocol::QuicV1);
    swarm.listen_on(listen_addr_quic_ipv4)?;
    swarm.listen_on(listen_addr_quic_ipv6)?;

    let rooms: Arc<RwLock<HashMap<String, (PeerId, PasswordHash)>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let hosts: Arc<RwLock<HashMap<PeerId, String>>> = Arc::new(RwLock::new(HashMap::new()));

    Ok(Relay {
        swarm,
        rooms,
        hosts,
    })
}

impl Relay {
    pub fn peer_id(&self) -> &PeerId {
        self.swarm.local_peer_id()
    }

    pub async fn run(&mut self) {
        loop {
            match self.swarm.next().await.expect("Infinite Stream.") {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    self.swarm.add_external_address(observed_addr.clone());
                    debug!("Added external node");
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    debug!(?cause, "Connection closed");
                    self.close_node(peer_id).await;
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    if let Some(pid) = peer_id {
                        self.close_node(pid).await;
                    }
                    debug!(?error, "Connection closed due to an error");
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message, .. },
                )) => match message {
                    request_response::Message::Request {
                        request, channel, ..
                    } => {
                        self.handle_init_request(peer, request, channel).await;
                    }
                    request_response::Message::Response { .. } => {
                        debug!("Received init response. This should not happen")
                    }
                },
                _ => {}
            }
        }
    }

    async fn close_node(&mut self, peer_id: PeerId) {
        let mut m = self.hosts.write().await;
        let mut r = self.rooms.write().await;
        if let Some(room) = m.get(&peer_id) {
            r.remove(room);
            m.remove(&peer_id);
        }
    }

    async fn handle_init_request(
        &mut self,
        peer: PeerId,
        request: InitRequest,
        channel: ResponseChannel<InitResponse>,
    ) {
        debug!("Received request from client");
        let mut r = self.rooms.write().await;
        let mut status = ResponseStatus::Ok;
        let mut peer_id: Option<PeerId> = None;
        if let Some((pid, req)) = r.get(request.room.as_str()) {
            if req.verify(request.password) {
                // host is available and password is correct
                if *pid != peer {
                    debug!("Authentication successfull");
                    peer_id = Some(*pid);
                }
            } else {
                debug!("Authentication failed");
                status = ResponseStatus::Err;
            }
        } else {
            // else no error and query client will be host
            debug!("Creating new room");
            let mut m = self.hosts.write().await;
            m.insert(peer, request.room.clone());
            r.insert(request.room.clone(), (peer, request.into()));
        }
        self.swarm
            .behaviour_mut()
            .init_request_response
            .send_response(channel, InitResponse { status, peer_id })
            .unwrap_or_default();
    }
}
