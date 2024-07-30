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

use std::collections::HashMap;
use std::error::Error;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use async_std::sync::RwLock;
use bcrypt::verify;
use clap::Parser;
use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Multiaddr;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identify, identity, noise, ping, relay, tcp, yamux, PeerId, StreamProtocol};
use log::{debug, info};
use serde::{Deserialize, Serialize};

fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();
    let opt = Opt::parse();

    let local_key: identity::Keypair = generate_ed25519(opt.secret_key_seed);

    //TODO build transport with short timeout
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
    info!("Finished initialization. Now receiving requests");

    let open_rooms: Arc<RwLock<HashMap<String, (PeerId, InitRequest)>>> =
        Arc::new(RwLock::new(HashMap::new()));
    let auth_nodes: Arc<RwLock<HashMap<PeerId, String>>> = Arc::new(RwLock::new(HashMap::new()));
    block_on(async {
        let map = auth_nodes.clone();
        let rooms = open_rooms.clone();

        loop {
            match swarm.next().await.expect("Infinite Stream.") {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    debug!("Added external node");
                    swarm.add_external_address(observed_addr.clone());
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    debug!("Connection closed due to {cause:?}");
                    let mut m = map.write().await;
                    let mut r = rooms.write().await;
                    if let Some(room) = m.get(&peer_id) {
                        r.remove(room);
                        m.remove(&peer_id);
                    }
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    let mut m = map.write().await;
                    let mut r = rooms.write().await;
                    if let Some(pid) = peer_id {
                        if let Some(room) = m.get(&pid) {
                            r.remove(room);
                            m.remove(&pid);
                        }
                    }
                    debug!("Outgoing connection closed due to {error:?}");
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message },
                )) => match message {
                    request_response::Message::Request {
                        request, channel, ..
                    } => {
                        debug!("Received request from client");
                        let mut r = rooms.write().await;
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
                            let mut m = map.write().await;
                            m.insert(peer, request.room.clone());
                            r.insert(request.room.clone(), (peer, request));
                        }
                        swarm
                            .behaviour_mut()
                            .init_request_response
                            .send_response(channel, InitResponse { status, peer_id })
                            .unwrap_or_default();
                    }
                    request_response::Message::Response { .. } => {
                        debug!("Received init response. This should not happen")
                    }
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
