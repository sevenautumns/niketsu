use std::collections::HashMap;

use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::{ConnectionError, ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostHandoverMsg, HostHandoverRequest, HostInstruction, IncomingMessage,
    OutgoingMessage, PeerConnectedMsg, PeerDisconnectedMsg, PeerStatusMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::room::RoomName;
use tracing::{debug, error, info, warn};

use super::auth::AuthEvent;
use super::links::{Closed, Links, Opened};
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, HostLink, MessageResponse, Stop, SwarmHandler,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct HostCommunicationHandler {
    handler: CommunicationHandler,
    relay_peer_id: PeerId,
    password: String,
    room: RoomName,
    /// The room's peers. The core hears of a peer while it is linked.
    links: Links,
    /// Where mDNS found a peer on the local network.
    lan_addrs: HashMap<PeerId, Multiaddr>,
    handover_pending: bool,
}

impl HostCommunicationHandler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_peer_id: PeerId,
        password: String,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        room: RoomName,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let handler = CommunicationHandler::new(
            swarm,
            topic,
            host,
            HostLink::Local,
            core_receiver,
            message_sender,
            chunk_routes,
        );
        Self {
            handler,
            relay_peer_id,
            password,
            room,
            links: Links::default(),
            lan_addrs: HashMap::default(),
            handover_pending: false,
        }
    }

    /// Dials a peer that waits on its circuit for a direct connection, if
    /// mDNS found it on the local network: no hole to punch there.
    fn dial_on_lan(&mut self, peer_id: PeerId) {
        if !self.links.awaits_direct(&peer_id) {
            return;
        }
        let Some(addr) = self.lan_addrs.get(&peer_id) else {
            return;
        };
        match self.handler.base.swarm.dial(addr.clone()) {
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
                    // dialed from on_connection_established.
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

    fn on_connection_established(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        if peer_id == self.relay_peer_id {
            return;
        }
        let swarm = &mut self.handler.base.swarm;
        // A circuit is the client's to close: the client gives up on a host
        // whose circuit closes before the direct connection shows up there.
        match self
            .links
            .opened(peer_id, connection_id, endpoint.is_relayed())
        {
            Opened::Rendezvous => {
                debug!(%peer_id, "Client connected via relay; awaiting direct connection");
                self.dial_on_lan(peer_id);
            }
            Opened::SpareCircuit => debug!(%peer_id, "Relay circuit beside a direct connection"),
            // A peer keeps its id when it reconnects, and the connections of
            // its previous session can outlive the start of the next one. So
            // any direct connection may be a peer starting over that waits
            // for the room's state, and each one is reported.
            Opened::Linked { .. } | Opened::AnotherDirect => {
                info!(%peer_id, ?endpoint, "Direct connection to client established");
                swarm
                    .kademlia()
                    .add_address(&peer_id, endpoint.get_remote_address().clone());
                swarm.gossipsub().add_explicit_peer(&peer_id);
                let res = self.handler.forward(PeerConnectedMsg { peer: peer_id });
                log_err_msg!(res, "Failed to report connected peer to core");
            }
        }
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        cause: Option<ConnectionError>,
    ) {
        if peer_id == self.relay_peer_id {
            // every connection to the relay is a direct one
            if !self.handler.base.swarm.is_connected(&peer_id) {
                error!(?cause, "Connection of host to relay server closed");
                self.handler.base.stop(Stop::RelayLost);
            }
            return;
        }
        match self.links.closed(peer_id, connection_id) {
            Closed::Unlinked => {
                debug!(%peer_id, ?cause, "Client disconnected");
                // Gossipsub redials disconnected explicit peers forever;
                // forget departed clients.
                let swarm = &mut self.handler.base.swarm;
                swarm.gossipsub().remove_explicit_peer(&peer_id);
                let res = self.handler.forward(PeerDisconnectedMsg { peer: peer_id });
                log_err_msg!(res, "Failed to report disconnected peer to core");
                // a circuit it left open is a peer starting over
                self.dial_on_lan(peer_id);
            }
            Closed::RendezvousLost => debug!(%peer_id, "Client left before a direct connection"),
            Closed::Unchanged => {}
        }
    }

    fn on_core_handover(&mut self, request: HostHandoverRequest) -> Result<()> {
        self.handler.base.swarm.auth().transfer(
            self.relay_peer_id,
            self.room.clone(),
            self.password.clone(),
            request.peer,
        );

        let announce = HostHandoverMsg {
            new_host: request.new_host,
        };
        self.handler.broadcast(announce.into())?;

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
            VideoStatus(m) => self.handler.broadcast(m.into()),
            Playlist(m) => self.handler.broadcast(m.into()),
            UserStatusList(m) => self.handler.broadcast(m.into()),
            ToPeer(m) => {
                self.handler
                    .base
                    .swarm
                    .send_request(&m.peer, m.message.into());
                Ok(())
            }
            HostHandover(m) => self.on_core_handover(m),
        }
    }

    fn on_auth(&mut self, event: AuthEvent) {
        if !self.handover_pending {
            return;
        }
        // The swarm kept running for the round trip to the relay, which gave
        // the handover broadcast time to leave this node as well. The room
        // is announced as handed over either way, so the session ends; the
        // core reconnects and takes the role the relay assigns.
        let stop = match event {
            AuthEvent::Complete { .. } => Stop::HandedOver,
            AuthEvent::Failed(error) => {
                error!(%error, "host handover failed");
                Stop::HandoverFailed
            }
        };
        self.handler.base.stop(stop);
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
impl CommunicationHandlerTrait for HostCommunicationHandler {
    fn handler_mut(&mut self) -> &mut CommunicationHandler {
        &mut self.handler
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(Transport(T::Auth(e))) => self.on_auth(e),
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => self.on_connection_established(peer_id, connection_id, endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                connection_id,
                cause,
                ..
            } => self.on_connection_closed(peer_id, connection_id, cause),
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
            Playlist(m) => self.handler.send_to_core(m, channel),
            Status(status) => {
                let msg = PeerStatusMsg {
                    peer: peer_id,
                    status,
                };
                self.handler.send_to_core(msg, channel)
            }
            other => self.handler.respond_with_err(other, channel),
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
                self.handler.reset_requests_responses();
                self.handler.forward(m)
            }
            m @ (Pause(_) | Start(_) | PlaybackSpeed(_) | Seek(_) | UserMessage(_)) => {
                self.handler.forward(m)
            }
            other => bail!("Host received unexpected broadcast message: {other:?}"),
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Result<()> {
        debug!(host = %self.handler.base.host, ?msg, "Handling core message");
        match msg {
            OutgoingMessage::Room(m) => self.handler.route_room_message(m),
            OutgoingMessage::Host(m) => self.on_core_instruction(m),
            OutgoingMessage::Client(m) => self.handler.drop_for_other_role(m),
        }
    }
}
