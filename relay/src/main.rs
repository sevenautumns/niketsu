// Copyright 2020 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![doc = include_str!("../README.md")]

use async_std::sync::RwLock;
use bcrypt::{hash, verify, DEFAULT_COST};
use clap::Parser;
use futures::stream::StreamExt;
use futures::{executor::block_on, stream::FusedStream};
use libp2p::relay::RequestId;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::{
    core::multiaddr::Protocol,
    core::Multiaddr,
    identify, identity, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opt = Opt::parse();

    let local_key: identity::Keypair = generate_ed25519(opt.secret_key_seed);

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
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

    println!("peer-id: {:?}", swarm.local_peer_id());

    // Listen on all interfaces
    let listen_addr_tcp = Multiaddr::empty()
        .with(match opt.use_ipv6 {
            Some(true) => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Tcp(opt.port));
    swarm.listen_on(listen_addr_tcp)?;

    let listen_addr_quic = Multiaddr::empty()
        .with(match opt.use_ipv6 {
            Some(true) => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Udp(opt.port))
        .with(Protocol::QuicV1);
    swarm.listen_on(listen_addr_quic)?;

    let mut rooms: Arc<RwLock<HashMap<String, (PeerId, InitRequest)>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let mut auth_nodes: Arc<RwLock<HashMap<PeerId, String>>> =
        Arc::new(RwLock::new(HashMap::new()));
    block_on(async {
        let map = auth_nodes.clone();
        loop {
            match swarm.next().await.expect("Infinite Stream.") {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    println!("Added external node");
                    swarm.add_external_address(observed_addr.clone());
                }
                SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                    peer,
                    connection,
                    result,
                })) => {
                    println!("Received ping from {peer:?} of connection {connection:?} with result {result:?}")
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {address:?}");
                }
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    connection_id,
                    endpoint,
                    num_established,
                    cause,
                } => {
                    let mut n = map.write().await;
                    let mut r = rooms.write().await;
                    if let Some(room) = n.get(&peer_id) {
                        r.remove(room);
                        n.remove(&peer_id);
                    }
                    println!("Connection closed by {peer_id:?}, cause {cause:?}");
                    println!("room {r:?}")
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message, .. },
                )) => match message {
                    request_response::Message::Request {
                        request, channel, ..
                    } => match request.intent {
                        0 => {
                            println!("Received request from listener: {request:?}");
                            let mut r = rooms.write().await;
                            println!("room: {r:?}");
                            if let Some(_) = r.get(request.room.as_str()) {
                                swarm
                                    .behaviour_mut()
                                    .init_request_response
                                    .send_response(
                                        channel,
                                        InitResponse {
                                            status: 1,
                                            peer_id: None,
                                        },
                                    )
                                    .unwrap_or_default();
                            } else {
                                let mut n = auth_nodes.write().await;
                                r.insert(request.room.clone(), (peer, request.clone()));
                                n.insert(peer, request.room);
                                swarm
                                    .behaviour_mut()
                                    .init_request_response
                                    .send_response(
                                        channel,
                                        InitResponse {
                                            status: 0,
                                            peer_id: None,
                                        },
                                    )
                                    .unwrap_or_default();
                            }
                        }
                        1 => {
                            println!("Received request from dialer: {request:?}");
                            let r = rooms.read().await;
                            println!("room: {r:?}");
                            if let Some((pid, req)) = r.get(request.room.as_str()) {
                                if !req.verify(request.password) {
                                    swarm
                                        .behaviour_mut()
                                        .init_request_response
                                        .send_response(
                                            channel,
                                            InitResponse {
                                                status: 1,
                                                peer_id: None,
                                            },
                                        )
                                        .unwrap_or_default();
                                } else {
                                    swarm
                                        .behaviour_mut()
                                        .init_request_response
                                        .send_response(
                                            channel,
                                            InitResponse {
                                                status: 0,
                                                peer_id: Some(*pid),
                                            },
                                        )
                                        .unwrap_or_default();
                                }
                            } else {
                                swarm
                                    .behaviour_mut()
                                    .init_request_response
                                    .send_response(
                                        channel,
                                        InitResponse {
                                            status: 1,
                                            peer_id: None,
                                        },
                                    )
                                    .unwrap_or_default();
                            }
                        }
                        _ => {}
                    },
                    request_response::Message::Response {
                        request_id,
                        response,
                    } => todo!(),
                },
                _ => {}
            }
        }
    })
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitRequest {
    intent: u8, // 0 -> listen, 1 -> dial
    room: String,
    password: String,
}

impl InitRequest {
    fn new(intent: u8, room: String, password: String) -> Self {
        Self {
            intent,
            room: room,
            password: hash(password, DEFAULT_COST).expect("Failed to hash password"),
        }
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

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}

#[derive(Debug, Parser)]
#[clap(name = "libp2p relay")]
struct Opt {
    /// Determine if the relay listen on ipv6 or ipv4 loopback address. the default is ipv4
    #[clap(long)]
    use_ipv6: Option<bool>,

    /// Fixed value to generate deterministic peer id
    #[clap(long)]
    secret_key_seed: u8,

    /// The port used to listen on all interfaces
    #[clap(long)]
    port: u16,
}
