use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, gossipsub, mdns, ping};
use niketsu_core::communicator::{
    ClientRequest, ConnectedMsg, HostRoundTripMsg, IncomingMessage, OutgoingMessage,
};
use tracing::{debug, info, warn};

use super::links::{Closed, Links, Opened};
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, HostLink, MessageResponse, Stop, SwarmHandler,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct ClientCommunicationHandler {
    handler: CommunicationHandler,
    /// The host and the peers this client shares files with.
    links: Links,
    /// Whether a playlist or a selection was broadcast to this peer. The
    /// host's greeting travels as direct requests, which a broadcast sent
    /// after it can overtake; the greeting is then the older state.
    heard_playlist: bool,
    heard_select: bool,
}

impl ClientCommunicationHandler {
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        // A client starts out with nothing but the circuit to the host being
        // dialed, and is connected once that led to a direct connection.
        let handler = CommunicationHandler::new(
            swarm,
            topic,
            host,
            HostLink::rendezvous(),
            core_receiver,
            message_sender,
            chunk_routes,
        );
        Self {
            handler,
            links: Links::default(),
            heard_playlist: false,
            heard_select: false,
        }
    }

    fn on_ping(&mut self, event: ping::Event) {
        let host = self.handler.base.host;
        if event.peer != host {
            return;
        }

        match event.result {
            // a circuit's round trip includes the detour over the relay
            Ok(rtt) if self.links.is_direct(&host, event.connection) => {
                if let Err(error) = self.handler.forward(HostRoundTripMsg { rtt }) {
                    warn!(%error, "Failed to report round trip to core");
                }
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "Ping to host failed, closing connection");
                self.handler.base.swarm.close_connection(event.connection);
            }
        }
    }

    fn on_dcutr(&mut self, event: dcutr::Event) {
        let host = self.handler.base.host;
        let peer = event.remote_peer_id;
        // A successful punch shows up as a direct connection, like any other
        // way of getting one.
        let Err(error) = event.result else {
            return;
        };
        warn!(%peer, %error, "Hole punching failed");
        // A punch is one of several ways to a direct connection (the host
        // also dials on the LAN), and a file-share peer is not needed for
        // the room. Without the host there is no room, though.
        if peer == host && !self.links.is_linked(&host) {
            self.handler.base.stop(Stop::HostUnreachable);
        }
    }

    fn on_mdns(&mut self, event: mdns::Event) {
        if let mdns::Event::Discovered(list) = event {
            for (peer_id, addr) in list {
                let swarm = &mut self.handler.base.swarm;
                swarm.kademlia().add_address(&peer_id, addr);
            }
        }
    }

    fn on_connection_established(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        let swarm = &mut self.handler.base.swarm;
        match self
            .links
            .opened(peer_id, connection_id, endpoint.is_relayed())
        {
            Opened::Rendezvous => debug!(%peer_id, "Relay circuit established"),
            Opened::SpareCircuit => {
                debug!(%peer_id, "Closing relay circuit, direct connection already exists");
                swarm.close_connection(connection_id);
            }
            Opened::Linked { circuits } => {
                info!(%peer_id, ?endpoint, "Direct connection established");
                for circuit in circuits {
                    swarm.close_connection(circuit);
                }
                swarm.gossipsub().add_explicit_peer(&peer_id);
                if peer_id == self.handler.base.host {
                    self.on_host_linked(endpoint.get_remote_address());
                }
            }
            Opened::AnotherDirect => debug!(%peer_id, "Another direct connection established"),
        }
    }

    /// The client is connected from here on.
    fn on_host_linked(&mut self, addr: &Multiaddr) {
        let base = &mut self.handler.base;
        base.swarm.kademlia().add_address(&base.host, addr.clone());
        base.host_link = HostLink::direct(addr);
        self.reserve_host_circuit();
        let connected = ConnectedMsg { is_host: false };
        if let Err(error) = self.handler.forward(connected) {
            warn!(%error, "Failed to send connected message to core");
        }
    }

    /// Reserves a slot on the host's relay so other clients can reach us at
    /// `<host circuit>/p2p/SELF` as the rendezvous for file-share hole
    /// punches. The relay client only makes reservations over non-relayed
    /// connections, and every `listen_on` of the circuit spawns another
    /// listener whose own reservation renewals count against the host
    /// relay's per-peer rate limit; hence once, when the host gets linked.
    fn reserve_host_circuit(&mut self) {
        let Some(host_circuit) = self.handler.base.host_circuit() else {
            return;
        };
        match self.handler.base.swarm.listen_on(host_circuit.clone()) {
            Ok(_) => info!(%host_circuit, "Reserving slot on host relay"),
            Err(err) => warn!(?err, %host_circuit, "Failed to listen on host relay circuit"),
        }
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        cause: Option<ConnectionError>,
        connection_id: ConnectionId,
    ) {
        let closed = self.links.closed(peer_id, connection_id);
        if peer_id == self.handler.base.host {
            let stop = match closed {
                Closed::Unlinked => Stop::HostLost,
                Closed::RendezvousLost => Stop::HostUnreachable,
                Closed::Unchanged => return,
            };
            warn!(?cause, %peer_id, %connection_id, "Connection to host closed");
            self.handler.base.stop(stop);
        } else if closed == Closed::Unlinked {
            // Gossipsub redials disconnected explicit peers forever; forget
            // departed file-share peers.
            let swarm = &mut self.handler.base.swarm;
            swarm.gossipsub().remove_explicit_peer(&peer_id);
        }
    }

    fn on_outgoing_connection_error(
        &mut self,
        connection_id: ConnectionId,
        peer_id: Option<PeerId>,
        error: DialError,
    ) {
        let host = self.handler.base.host;
        if peer_id == Some(host) && !self.links.is_connected(&host) {
            warn!(%error, %host, %connection_id, "Failed to reach the host");
            self.handler.base.stop(Stop::HostUnreachable);
        } else {
            debug!(%error, ?peer_id, %connection_id, "Outgoing connection error");
        }
    }

    fn send_to_host(&mut self, msg: NiketsuMessage) -> Result<()> {
        let host = self.handler.base.host;
        self.handler.base.swarm.send_request(&host, msg);
        Ok(())
    }
}

#[async_trait]
impl CommunicationHandlerTrait for ClientCommunicationHandler {
    fn handler_mut(&mut self) -> &mut CommunicationHandler {
        &mut self.handler
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(Transport(T::Ping(e))) => self.on_ping(e),
            SwarmEvent::Behaviour(Transport(T::Dcutr(e))) => self.on_dcutr(e),
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => self.on_connection_established(peer_id, connection_id, endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => self.on_connection_closed(peer_id, cause, connection_id),
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            } => self.on_outgoing_connection_error(connection_id, peer_id, error),
            other => debug!(event = ?other, "Received not captured event"),
        }
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!("Received swarm request {msg:?}");
        if peer_id != self.handler.base.host {
            return self.handler.respond_with_err(msg, channel);
        }
        let overtaken = match msg {
            NiketsuMessage::Playlist(_) => self.heard_playlist,
            NiketsuMessage::Select(_) => self.heard_select,
            _ => false,
        };
        if overtaken {
            debug!("Dropping the greeting's state, a broadcast was newer");
            return self.handler.acknowledge(channel);
        }
        self.handler.send_to_core(msg, channel)
    }

    fn handle_swarm_broadcast(
        &mut self,
        niketsu_msg: NiketsuMessage,
        source: PeerId,
    ) -> Result<()> {
        debug!(message = ?niketsu_msg, %source, "Received broadcast");
        use NiketsuMessage::*;
        match niketsu_msg {
            // only the host's clock is a reference for the room
            VideoStatus(_) if source != self.handler.base.host => {
                bail!("Received video status from non-host peer: {source:?}")
            }
            Select(msg) => {
                self.heard_select = true;
                self.handler.reset_requests_responses();
                self.handler.forward(msg)
            }
            Playlist(msg) => {
                self.heard_playlist = true;
                self.handler.forward(msg)
            }
            other => self.handler.forward(other),
        }
    }

    fn on_msg_req_resp_other(
        &mut self,
        event: request_response::Event<super::MessageRequest, MessageResponse>,
    ) {
        match event {
            request_response::Event::OutboundFailure { peer, error, .. } => {
                let host = self.handler.base.host;
                if peer == host {
                    warn!(%error, %peer, %host, "Outbound failure for request response");
                }
            }
            other => debug!(
                ?other,
                "Received request response event that is not handled"
            ),
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Result<()> {
        debug!(?msg, host = %self.handler.base.host, peer = %self.handler.base.swarm.local_peer_id(), "Handling core message");
        match msg {
            OutgoingMessage::Room(m) => self.handler.route_room_message(m),
            OutgoingMessage::Client(ClientRequest::Playlist(m)) => self.send_to_host(m.into()),
            OutgoingMessage::Client(ClientRequest::UserStatus(m)) => self.send_to_host(m.into()),
            OutgoingMessage::Host(m) => self.handler.drop_for_other_role(m),
        }
    }
}
