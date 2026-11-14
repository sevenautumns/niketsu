use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::Result;
use bcrypt::{DEFAULT_COST, hash, verify};
use futures::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::core::multiaddr::Protocol;
use libp2p::identity::Keypair;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{PeerId, StreamProtocol, Swarm, identify, noise, ping, relay, tcp, yamux};
use niketsu_protocol::{AUTH_PROTOCOL, AuthRequest, AuthResponse, AuthStatus};
use tracing::debug;

use crate::timing::{
    CIRCUIT_DURATION, IDLE_CONNECTION_TIMEOUT, PING_INTERVAL, PING_TIMEOUT, QUIC_HANDSHAKE_TIMEOUT,
    QUIC_IDLE_TIMEOUT,
};

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    auth: request_response::cbor::Behaviour<AuthRequest, AuthResponse>,
}

struct PasswordHash(String);

impl PasswordHash {
    /// None for a digest bcrypt refuses to hash.
    fn new(digest: &str) -> Option<Self> {
        hash(digest, DEFAULT_COST).ok().map(Self)
    }

    fn verify(&self, digest: &str) -> bool {
        verify(digest, &self.0).unwrap_or(false)
    }
}

fn granted(host: Option<PeerId>) -> AuthResponse {
    AuthResponse {
        status: AuthStatus::Ok,
        peer_id: host,
    }
}

fn denied() -> AuthResponse {
    AuthResponse {
        status: AuthStatus::Err,
        peer_id: None,
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
    /// Answers `peer` asking for a room: it joins the room with its
    /// password and learns who hosts it, or opens it when nobody does.
    fn answer(&mut self, peer: PeerId, request: AuthRequest) -> AuthResponse {
        let AuthRequest { room, password } = request;

        let Some(open) = self.get(&room) else {
            let Some(password) = PasswordHash::new(&password) else {
                debug!(%peer, %room, "Refused to open room: unusable password digest");
                return denied();
            };
            debug!(%peer, %room, "Opened room");
            self.open(room, peer, password);
            return granted(None);
        };
        if !open.password.verify(&password) {
            debug!(%peer, %room, "Refused to join room: wrong password");
            return denied();
        }
        debug!(%peer, %room, host = %open.host, "Joined room");
        // A host asking for its own room is told of no other host.
        granted(Some(open.host).filter(|host| *host != peer))
    }

    fn get(&self, name: &str) -> Option<&Room> {
        self.rooms.get(name)
    }

    fn open(&mut self, name: RoomName, host: PeerId, password: PasswordHash) {
        self.close_hosted_by(host);
        self.hosts.insert(host, name.clone());
        self.rooms.insert(name, Room { host, password });
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

impl Relay {
    /// A relay that listens on `port` on every interface, over TCP and QUIC.
    pub fn new(keypair: Keypair, port: u16) -> Result<Self> {
        let mut quic_config = libp2p::quic::Config::new(&keypair.clone());
        quic_config.handshake_timeout = QUIC_HANDSHAKE_TIMEOUT;
        quic_config.max_idle_timeout = QUIC_IDLE_TIMEOUT.as_millis() as u32;

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
                    // nothing but identify/DCUtR/ping ever flows here, for no
                    // longer than a client waits for its punch (see
                    // CIRCUIT_DURATION). The byte cap guards against peers
                    // that don't play by DirectOnly's rules.
                    relay::Config {
                        max_circuits: 128,
                        max_circuits_per_peer: 4,
                        max_circuit_duration: CIRCUIT_DURATION,
                        max_circuit_bytes: 128 * 1024,
                        ..Default::default()
                    },
                ),
                ping: ping::Behaviour::new(
                    ping::Config::new()
                        .with_interval(PING_INTERVAL)
                        .with_timeout(PING_TIMEOUT),
                ),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/identify/1".to_string(),
                    key.public(),
                )),
                auth: request_response::cbor::Behaviour::new(
                    [(StreamProtocol::new(AUTH_PROTOCOL), ProtocolSupport::Full)],
                    request_response::Config::default(),
                ),
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(IDLE_CONNECTION_TIMEOUT))
            .build();

        for ip in [
            IpAddr::from(Ipv4Addr::UNSPECIFIED),
            IpAddr::from(Ipv6Addr::UNSPECIFIED),
        ] {
            let ip = Multiaddr::from(ip);
            swarm.listen_on(ip.clone().with(Protocol::Tcp(port)))?;
            swarm.listen_on(ip.with(Protocol::Udp(port)).with(Protocol::QuicV1))?;
        }

        Ok(Self {
            swarm,
            rooms: Rooms::default(),
        })
    }

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
                    debug!(%observed_addr, "Added external address");
                    self.swarm.add_external_address(observed_addr);
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
                SwarmEvent::Behaviour(BehaviourEvent::Auth(request_response::Event::Message {
                    peer,
                    message,
                    ..
                })) => match message {
                    request_response::Message::Request {
                        request, channel, ..
                    } => {
                        let response = self.rooms.answer(peer, request);
                        let auth = &mut self.swarm.behaviour_mut().auth;
                        if auth.send_response(channel, response).is_err() {
                            debug!(%peer, "Peer left before its auth request was answered");
                        }
                    }
                    request_response::Message::Response { .. } => {
                        debug!(%peer, "Ignored an auth response: the relay asks nothing");
                    }
                },
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password() -> PasswordHash {
        PasswordHash(String::new())
    }

    fn ask(room: &str, password: &str) -> AuthRequest {
        AuthRequest {
            room: room.into(),
            password: password.into(),
        }
    }

    #[test]
    fn a_room_names_its_host_to_those_with_its_password() {
        let (alice, bob) = (PeerId::random(), PeerId::random());
        let mut rooms = Rooms::default();
        assert_eq!(rooms.answer(alice, ask("room", "secret")), granted(None));
        assert_eq!(rooms.answer(bob, ask("room", "guess")), denied());
        assert_eq!(
            rooms.answer(bob, ask("room", "secret")),
            granted(Some(alice))
        );
        assert_eq!(rooms.answer(alice, ask("room", "secret")), granted(None));
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
