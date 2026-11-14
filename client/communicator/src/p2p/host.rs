use std::collections::HashMap;

use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::{ConnectionError, ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostInstruction, IncomingMessage, OutgoingMessage, PeerConnectedMsg,
    PeerDisconnectedMsg, PeerIntentMsg,
};
use niketsu_core::log_err_msg;
use tracing::{debug, error, info, warn};

use super::links::{Closed, Opened};
use super::member::Membership;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, FileShareBehaviourEvent, HostLink, MessageResponse,
    Role, Session, Stop, SwarmExt,
};
use crate::messages::NiketsuMessage;

pub(crate) struct HostRole {
    session: Session,
    relay_peer_id: PeerId,
    /// The listener on the relay's circuit, which lives as long as the
    /// reservation there does. Peers reach the host through it.
    reservation: ListenerId,
    /// Where mDNS found a peer on the local network.
    lan_addrs: HashMap<PeerId, Multiaddr>,
}

impl HostRole {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        membership: Membership,
        host: PeerId,
        relay_peer_id: PeerId,
        reservation: ListenerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let session = Session::new(
            swarm,
            topic,
            membership,
            host,
            HostLink::Local,
            core_receiver,
            message_sender,
            chunk_routes,
        );
        Self {
            session,
            relay_peer_id,
            reservation,
            lan_addrs: HashMap::default(),
        }
    }

    /// Dials a peer that waits on its circuit for a direct connection, if
    /// mDNS found it on the local network: no hole to punch there.
    fn dial_on_lan(&mut self, peer_id: PeerId) {
        if !self.session.links.awaits_direct(&peer_id) {
            return;
        }
        let Some(addr) = self.lan_addrs.get(&peer_id) else {
            return;
        };
        match self.session.net.swarm.dial(addr.clone()) {
            Ok(()) => debug!(%peer_id, %addr, "Dialing peer on the local network"),
            Err(err) => warn!(%peer_id, %addr, ?err, "Failed to dial peer on the local network"),
        }
    }

    fn on_mdns(&mut self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(nodes) => {
                debug!(?nodes, "mDNS discovered some nodes");
                // mDNS reports every local address of a peer; one dial is
                // enough.
                let mut preferred: HashMap<PeerId, Multiaddr> = HashMap::new();
                for (peer, addr) in nodes {
                    match preferred.get(&peer) {
                        Some(known) if lan_rank(known) <= lan_rank(&addr) => {}
                        _ => {
                            preferred.insert(peer, addr);
                        }
                    }
                }
                for (peer, addr) in preferred {
                    // A peer that has not dialed the host's circuit yet is
                    // dialed from on_opened.
                    self.lan_addrs.insert(peer, addr);
                    self.dial_on_lan(peer);
                }
            }
            mdns::Event::Expired(nodes) => {
                for (peer, _) in nodes {
                    debug!(%peer, "mDNS node expired");
                    self.lan_addrs.remove(&peer);
                }
            }
        }
    }

    /// Takes a peer that asked to join into the room. A peer joins again
    /// with each session, also while connections of its previous one are
    /// open, and is greeted each time.
    fn welcome(&mut self, peer_id: PeerId) {
        let swarm = &mut self.session.net.swarm;
        swarm.gossipsub().add_explicit_peer(&peer_id);
        let res = self.session.forward(PeerConnectedMsg { peer: peer_id });
        log_err_msg!(res, "Failed to report connected peer to core");
    }

    fn on_core_instruction(&mut self, instruction: HostInstruction) -> Result<()> {
        use HostInstruction::*;
        match instruction {
            RoomState(m) => self.session.broadcast(m.into()),
            Action(m) => self.session.broadcast(m.into()),
            ToPeer(m) => {
                self.session.net.send_request(&m.peer, m.message.into());
                Ok(())
            }
        }
    }
}

/// Which of a peer's local addresses to dial first: TCP, then QUIC.
fn lan_rank(addr: &Multiaddr) -> u8 {
    if addr.iter().any(|p| matches!(p, Protocol::Tcp(_))) {
        0
    } else if addr.iter().any(|p| matches!(p, Protocol::QuicV1)) {
        1
    } else {
        2
    }
}

#[async_trait]
impl Role for HostRole {
    fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    fn on_opened(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        endpoint: ConnectedPoint,
        opened: Opened,
    ) {
        if peer_id == self.relay_peer_id {
            return;
        }
        // A circuit is the client's to close: the client gives up on a host
        // whose circuit closes before the direct connection shows up there.
        // A connection makes no member; a peer joins with a request.
        match opened {
            Opened::Rendezvous => {
                debug!(%peer_id, "Client connected via relay; awaiting direct connection");
                self.dial_on_lan(peer_id);
            }
            Opened::SpareCircuit => debug!(%peer_id, "Relay circuit beside a direct connection"),
            Opened::Linked { .. } | Opened::AnotherDirect => {
                info!(%peer_id, ?endpoint, "Direct connection established");
                let swarm = &mut self.session.net.swarm;
                swarm
                    .kademlia()
                    .add_address(&peer_id, endpoint.get_remote_address().clone());
            }
        }
    }

    fn on_closed(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        cause: Option<ConnectionError>,
        closed: Closed,
    ) {
        if peer_id == self.relay_peer_id {
            // The swarm is asked, as the connection to the relay is older
            // than the session and its links. Every connection to the relay
            // is a direct one.
            if !self.session.net.swarm.is_connected(&peer_id) {
                error!(?cause, "Connection of host to relay server closed");
                self.session.net.stop(Stop::RelayLost);
            }
            return;
        }
        match closed {
            // the core forgets a peer that joined, and ignores one that never
            // did
            Closed::Unlinked => {
                debug!(%peer_id, ?cause, "Peer disconnected");
                self.session.fs_peer_unlinked(peer_id);
                // Gossipsub redials disconnected explicit peers forever;
                // forget departed clients.
                let swarm = &mut self.session.net.swarm;
                swarm.gossipsub().remove_explicit_peer(&peer_id);
                let res = self.session.forward(PeerDisconnectedMsg { peer: peer_id });
                log_err_msg!(res, "Failed to report disconnected peer to core");
                // a circuit it left open is a peer starting over
                self.dial_on_lan(peer_id);
            }
            Closed::RendezvousLost => debug!(%peer_id, "Client left before a direct connection"),
            Closed::Unchanged => {}
        }
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        match event {
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            // Without the reservation nobody can join the room, also when
            // the connection to the relay is still up.
            SwarmEvent::ListenerClosed {
                listener_id,
                reason,
                ..
            } if listener_id == self.reservation => {
                error!(?reason, "Reservation of host on relay server closed");
                self.session.net.stop(Stop::RelayLost);
            }
            other => debug!(event = ?other, "Received not captured event"),
        }
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Handling request message from swarm");
        use NiketsuMessage::*;
        match msg {
            Join => {
                self.welcome(peer_id);
                self.session.acknowledge(channel)
            }
            Intent(intent) => {
                let msg = PeerIntentMsg {
                    peer: peer_id,
                    intent,
                };
                self.session.send_to_core(msg, channel)
            }
            other => self.session.respond_with_err(other, channel),
        }
    }

    fn handle_swarm_broadcast(
        &mut self,
        niketsu_msg: NiketsuMessage,
        source: PeerId,
    ) -> Result<()> {
        debug!(message = ?niketsu_msg, %source, "Handling broadcast message from swarm");
        match niketsu_msg {
            NiketsuMessage::UserMessage(m) => self.session.forward(m),
            other => bail!("Host received unexpected broadcast message: {other:?}"),
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Result<()> {
        debug!(host = %self.session.net.host, ?msg, "Handling core message");
        match msg {
            OutgoingMessage::Room(m) => self.session.route_room_message(m),
            OutgoingMessage::Host(m) => self.on_core_instruction(m),
            OutgoingMessage::Client(m) => self.session.drop_for_other_role(m),
        }
    }
}
