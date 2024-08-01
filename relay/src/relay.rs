use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use crate::config::Config;
use anyhow::Result;
use async_std::stream::StreamExt;
use async_std::sync::RwLock;
use bcrypt::verify;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Multiaddr;
use libp2p::identity::Keypair;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identify, noise, ping, relay, tcp, yamux, PeerId, StreamProtocol, Swarm};
use log::debug;
use serde::{Deserialize, Serialize};

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    // should be hashed when listening
    room: String,
    password: String,
}

impl InitRequest {
    fn verify(&self, password: String) -> bool {
        verify(password, &self.password).unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitResponse {
    status: u8,              // 0 -> ok, 1 -> err
    peer_id: Option<PeerId>, // peer id of room if found
}

pub struct Relay {
    swarm: Swarm<Behaviour>,
    rooms: Arc<RwLock<HashMap<String, (PeerId, InitRequest)>>>,
    hosts: Arc<RwLock<HashMap<PeerId, String>>>,
}

pub fn new(config: Config) -> Result<Relay> {
    //TODO build transport with short timeout
    let keypair = Keypair::from_protobuf_encoding(config.keypair.unwrap().as_slice())?;
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_async_std()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| Behaviour {
            relay: relay::Behaviour::new(key.public().to_peer_id(), Default::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
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
        .build();

    let listen_addr_tcp = Multiaddr::empty()
        .with(match config.ipv6 {
            true => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Tcp(config.port));
    swarm.listen_on(listen_addr_tcp)?;

    let listen_addr_quic = Multiaddr::empty()
        .with(match config.ipv6 {
            true => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Udp(config.port))
        .with(Protocol::QuicV1);
    swarm.listen_on(listen_addr_quic)?;

    let rooms: Arc<RwLock<HashMap<String, (PeerId, InitRequest)>>> =
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
                    self.close_node(peer_id).await;
                    debug!("Connection closed due to: {cause:?}");
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    if let Some(pid) = peer_id {
                        self.close_node(pid).await;
                    }
                    debug!("Connection closed due to an error: {error:?}");
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message },
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
        let mut status: u8 = 0;
        let mut peer_id: Option<PeerId> = None;
        if let Some((pid, req)) = r.get(request.room.as_str()) {
            if req.verify(request.password) {
                // host is available and password is correct
                if *pid != peer {
                    debug!("Verified password. Returning pid");
                    peer_id = Some(*pid);
                }
            } else {
                debug!("Auth failed");
                status = 1;
            }
        } else {
            // else no error and query client will be host
            debug!("Creating new room");
            let mut m = self.hosts.write().await;
            m.insert(peer, request.room.clone());
            r.insert(request.room.clone(), (peer, request));
        }
        self.swarm
            .behaviour_mut()
            .init_request_response
            .send_response(channel, InitResponse { status, peer_id })
            .unwrap_or_default();
    }
}