use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use anyhow::Result;
use bcrypt::{DEFAULT_COST, hash, verify};
use futures::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::core::multiaddr::Protocol;
use libp2p::identity::Keypair;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{PeerId, StreamProtocol, Swarm, identify, noise, ping, relay, tcp, yamux};
use niketsu_protocol::{AUTH_PROTOCOL, AuthStatus, InitRequest, InitResponse};
use tracing::debug;

use crate::config::Config;

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
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

struct Room {
    host: PeerId,
    password: PasswordHash,
}

/// The open rooms and who hosts them. A peer hosts at most one room, and
/// `hosts` is the reverse of `rooms`.
#[derive(Default)]
struct Rooms {
    rooms: HashMap<RoomName, Room>,
    hosts: HashMap<PeerId, RoomName>,
}

impl Rooms {
    fn get(&self, name: &str) -> Option<&Room> {
        self.rooms.get(name)
    }

    fn open(&mut self, name: RoomName, host: PeerId, password: PasswordHash) {
        self.close_hosted_by(host);
        self.hosts.insert(host, name.clone());
        self.rooms.insert(name, Room { host, password });
    }

    /// Hands `name` from its host `from` over to `to`. False if `from` does
    /// not host it.
    fn transfer(&mut self, name: &str, from: PeerId, to: PeerId) -> bool {
        if self.get(name).is_none_or(|room| room.host != from) {
            return false;
        }
        let Some(Room { password, .. }) = self.rooms.remove(name) else {
            return false;
        };
        self.hosts.remove(&from);
        self.open(name.to_owned(), to, password);
        true
    }

    fn close_hosted_by(&mut self, host: PeerId) {
        if let Some(name) = self.hosts.remove(&host) {
            self.rooms.remove(&name);
        }
    }
}

pub struct Relay {
    swarm: Swarm<Behaviour>,
    rooms: Rooms,
}

pub fn new(config: Config) -> Result<Relay> {
    let keypair = Keypair::from_protobuf_encoding(config.keypair.unwrap().as_slice())?;
    let mut quic_config = libp2p::quic::Config::new(&keypair.clone());
    quic_config.handshake_timeout = Duration::from_secs(10);
    quic_config.max_idle_timeout = 3 * 1000;

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
                // Circuits through the public relay are only a hole-punch
                // rendezvous: the client keeps all niketsu protocols off
                // relayed connections (see DirectOnly in the communicator), so
                // nothing but identify/DCUtR/ping ever flows here. Clients
                // abort a punch after 30 s, so any older circuit is dead
                // weight; the duration cap only needs to stay above that
                // client deadline. The byte cap guards against peers that
                // don't play by DirectOnly's rules.
                relay::Config {
                    max_circuits: 128,
                    max_circuits_per_peer: 4,
                    max_circuit_duration: Duration::from_secs(60),
                    max_circuit_bytes: 128 * 1024,
                    ..Default::default()
                },
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
                [(StreamProtocol::new(AUTH_PROTOCOL), ProtocolSupport::Full)],
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

    Ok(Relay {
        swarm,
        rooms: Rooms::default(),
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
                // A host is gone with its last connection; a peer may hold
                // several, e.g. over TCP and QUIC.
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    cause,
                    num_established,
                    ..
                } => {
                    debug!(%peer_id, ?cause, num_established, "Connection closed");
                    if num_established == 0 {
                        self.rooms.close_hosted_by(peer_id);
                    }
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    debug!(?peer_id, ?error, "Outgoing connection failed");
                    if let Some(peer_id) = peer_id
                        && !self.swarm.is_connected(&peer_id)
                    {
                        self.rooms.close_hosted_by(peer_id);
                    }
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message, .. },
                )) => match message {
                    request_response::Message::Request {
                        request, channel, ..
                    } => {
                        self.handle_init_request(peer, request, channel);
                    }
                    request_response::Message::Response { .. } => {
                        debug!("Received init response. This should not happen")
                    }
                },
                _ => {}
            }
        }
    }

    fn handle_init_request(
        &mut self,
        peer: PeerId,
        request: InitRequest,
        channel: ResponseChannel<InitResponse>,
    ) {
        debug!("Received request from client");

        if let Some(new_host) = request.transfer_to {
            // Only the current host of the room may transfer it.
            let mut status = AuthStatus::Err;
            if self.rooms.transfer(&request.room, peer, new_host) {
                debug!(%peer, %new_host, room = %request.room, "Host transfer completed");
                status = AuthStatus::Ok;
            }
            self.swarm
                .behaviour_mut()
                .init_request_response
                .send_response(
                    channel,
                    InitResponse {
                        status,
                        peer_id: None,
                    },
                )
                .unwrap_or_default();
            return;
        }

        let mut status = AuthStatus::Ok;
        let mut peer_id: Option<PeerId> = None;
        if let Some(room) = self.rooms.get(&request.room) {
            if room.password.verify(request.password) {
                // host is available and password is correct
                if room.host != peer {
                    debug!("Authentication successfull");
                    peer_id = Some(room.host);
                }
            } else {
                debug!("Authentication failed");
                status = AuthStatus::Err;
            }
        } else {
            // else no error and query client will be host
            debug!("Creating new room");
            let name = request.room.clone();
            self.rooms.open(name, peer, request.into());
        }
        self.swarm
            .behaviour_mut()
            .init_request_response
            .send_response(channel, InitResponse { status, peer_id })
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password() -> PasswordHash {
        PasswordHash(String::new())
    }

    #[test]
    fn a_transferred_room_closes_with_its_new_host() {
        let (alice, bob) = (PeerId::random(), PeerId::random());
        let mut rooms = Rooms::default();
        rooms.open("room".into(), alice, password());
        assert!(!rooms.transfer("room", bob, alice));
        assert!(rooms.transfer("room", alice, bob));
        rooms.close_hosted_by(alice);
        assert_eq!(rooms.get("room").map(|r| r.host), Some(bob));
        rooms.close_hosted_by(bob);
        assert!(rooms.get("room").is_none());
    }

    #[test]
    fn a_host_opening_another_room_leaves_the_first() {
        let alice = PeerId::random();
        let mut rooms = Rooms::default();
        rooms.open("first".into(), alice, password());
        rooms.open("second".into(), alice, password());
        assert!(rooms.get("first").is_none());
        rooms.close_hosted_by(alice);
        assert!(rooms.get("second").is_none());
    }
}
