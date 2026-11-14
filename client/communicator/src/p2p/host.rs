use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::{ConnectionError, ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostHandoverRequest, HostInstruction, IncomingMessage, OutgoingMessage,
    PeerConnectedMsg, PeerDisconnectedMsg, PeerStatusMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::room::RoomName;
use tracing::{debug, error, info, warn};

use super::auth::AuthEvent;
use super::links::{Closed, Opened};
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, FileShareBehaviourEvent, HostLink, MessageResponse,
    Role, Session, Stop, SwarmExt, TransportBehaviourEvent,
};
use crate::messages::{Handover, NiketsuMessage};

pub(crate) struct HostRole {
    session: Session,
    relay_peer_id: PeerId,
    /// The listener on the relay's circuit, which lives as long as the
    /// reservation there does. Peers join the room through it.
    reservation: ListenerId,
    password: String,
    room: RoomName,
    /// The peers that joined the room through the host's circuit on the
    /// relay, which the relay names only to peers that know the room's
    /// password. A direct connection alone makes no member: mDNS also
    /// finds peers of other rooms. A member stays one, as its circuit may
    /// close before its direct connection shows up here.
    members: HashSet<PeerId>,
    /// Where mDNS found a peer on the local network.
    lan_addrs: HashMap<PeerId, Multiaddr>,
    handover_pending: bool,
}

impl HostRole {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_peer_id: PeerId,
        reservation: ListenerId,
        password: String,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        room: RoomName,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let session = Session::new(
            swarm,
            topic,
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
            password,
            room,
            members: HashSet::default(),
            lan_addrs: HashMap::default(),
            handover_pending: false,
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

    fn is_member(&self, peer_id: &PeerId) -> bool {
        self.members.contains(peer_id)
    }

    /// Takes a linked member into the room.
    fn welcome(&mut self, peer_id: PeerId) {
        let swarm = &mut self.session.net.swarm;
        swarm.gossipsub().add_explicit_peer(&peer_id);
        let res = self.session.forward(PeerConnectedMsg { peer: peer_id });
        log_err_msg!(res, "Failed to report connected peer to core");
    }

    fn on_core_handover(&mut self, request: HostHandoverRequest) -> Result<()> {
        self.session.net.swarm.auth().transfer(
            self.relay_peer_id,
            self.room.clone(),
            self.password.clone(),
            request.peer,
        );

        let announce = Handover {
            peer: request.peer,
            new_host: request.new_host,
        };
        self.session.broadcast(announce.into())?;

        // Both the transfer request and the broadcast are only queued in the
        // behaviours at this point. Stopping now would drop the swarm before
        // they are sent, so the loop runs on until the relay answers the
        // transfer request (see on_auth).
        self.handover_pending = true;
        Ok(())
    }

    fn on_core_instruction(&mut self, instruction: HostInstruction) -> Result<()> {
        use HostInstruction::*;
        match instruction {
            VideoStatus(m) => self.session.broadcast(m.into()),
            Playlist(m) => self.session.broadcast(m.into()),
            UserStatusList(m) => self.session.broadcast(m.into()),
            ToPeer(m) => {
                let swarm = &mut self.session.net.swarm;
                swarm.send_request(&m.peer, m.message.into());
                Ok(())
            }
            HostHandover(m) => self.on_core_handover(m),
        }
    }

    fn on_auth(&mut self, event: AuthEvent) {
        if !self.handover_pending {
            return;
        }
        // The room is announced as handed over either way, so the session
        // ends; the core reconnects and takes the role the relay assigns.
        let stop = match event {
            AuthEvent::Complete { .. } => Stop::HandedOver,
            AuthEvent::Failed(error) => {
                error!(%error, "host handover failed");
                Stop::HandoverFailed
            }
        };
        self.session.net.stop(stop);
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
        let was_member = self.is_member(&peer_id);
        if endpoint.is_relayed() {
            self.members.insert(peer_id);
        }
        // A circuit is the client's to close: the client gives up on a host
        // whose circuit closes before the direct connection shows up there.
        match opened {
            Opened::Rendezvous => {
                debug!(%peer_id, "Client connected via relay; awaiting direct connection");
                self.dial_on_lan(peer_id);
            }
            Opened::SpareCircuit => {
                debug!(%peer_id, "Relay circuit beside a direct connection");
                if !was_member {
                    self.welcome(peer_id);
                }
            }
            // A peer keeps its id when it reconnects, and the connections of
            // its previous session can outlive the start of the next one. So
            // any direct connection may be a peer starting over that waits
            // for the room's state, and each one is reported.
            Opened::Linked { .. } | Opened::AnotherDirect => {
                info!(%peer_id, ?endpoint, "Direct connection established");
                let swarm = &mut self.session.net.swarm;
                swarm
                    .kademlia()
                    .add_address(&peer_id, endpoint.get_remote_address().clone());
                if was_member {
                    self.welcome(peer_id);
                } else {
                    debug!(%peer_id, "Peer has not joined the room");
                }
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
        let was_member = self.is_member(&peer_id);
        if closed == Closed::Unlinked {
            self.session.fs_peer_unlinked(peer_id);
        }
        match closed {
            Closed::Unlinked if !was_member => debug!(%peer_id, "Bystander disconnected"),
            Closed::Unlinked => {
                debug!(%peer_id, ?cause, "Client disconnected");
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
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(Transport(T::Auth(e))) => self.on_auth(e),
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
        if !self.is_member(&peer_id) {
            return self.session.respond_with_err(msg, channel);
        }
        match msg {
            Playlist(m) => self.session.send_to_core(m, channel),
            Status(status) => {
                let msg = PeerStatusMsg {
                    peer: peer_id,
                    status,
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
        use NiketsuMessage::*;
        match niketsu_msg {
            Select(m) => {
                self.session.fs_end();
                self.session.forward(m)
            }
            m @ (Pause(_) | Start(_) | PlaybackSpeed(_) | Seek(_) | UserMessage(_)) => {
                self.session.forward(IncomingMessage::try_from(m)?)
            }
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
