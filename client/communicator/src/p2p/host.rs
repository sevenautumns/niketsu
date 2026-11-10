use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::{ConnectionError, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostHandoverMsg, HostHandoverRequest, IncomingMessage, OutgoingMessage,
    PeerConnectedMsg, PeerDisconnectedMsg, PeerStatusMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::room::RoomName;
use tracing::{debug, error, warn};

use super::auth::AuthEvent;
use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, MessageResponse, SwarmHandler, TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

/// How a room peer is reachable from the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerLink {
    /// Only via the relay circuit, which carries hole-punch coordination
    /// but no room messages (see DirectOnly).
    Relayed,
    /// Over a direct connection.
    Direct,
}

pub(crate) struct HostCommunicationHandler {
    handler: CommunicationHandler,
    relay_peer_id: PeerId,
    password: String,
    room: RoomName,
    peers: HashMap<PeerId, PeerLink>,
    mdns_users: HashMap<PeerId, Multiaddr>,
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
    ) -> Self {
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let handler = CommunicationHandler::new(swarm, topic, host, core_receiver, message_sender);
        Self {
            handler,
            relay_peer_id,
            password,
            room,
            peers: HashMap::default(),
            mdns_users: HashMap::default(),
            handover_pending: false,
        }
    }

    /// A peer that joined via the relay and still waits for a direct
    /// connection, which a LAN dial can provide.
    fn awaits_direct(&self, peer_id: &PeerId) -> bool {
        self.peers.get(peer_id) == Some(&PeerLink::Relayed)
    }

    fn dial_peer(&mut self, peer_id: PeerId, addr: &Multiaddr) -> Result<()> {
        if let Err(err) = self.handler.base.swarm.dial(addr.clone()) {
            warn!(?peer_id, ?err, "Failed to dial mDNS node");
            bail!("Failed to dial mDNS node");
        } else {
            debug!(?peer_id, "Dialing mDNS node");

            let kad = &mut self
                .handler
                .base
                .swarm
                .behaviour_mut()
                .file_share
                .inner_mut()
                .kademlia;
            kad.add_address(&peer_id, addr.clone());

            let gossip = &mut self
                .handler
                .base
                .swarm
                .behaviour_mut()
                .messaging
                .inner_mut()
                .gossipsub;
            gossip.add_explicit_peer(&peer_id);
        }
        Ok(())
    }

    fn dial_on_new_connection(&mut self, peer_id: PeerId) -> Result<()> {
        let Some(addr) = self.mdns_users.get(&peer_id).cloned() else {
            debug!(?peer_id, "peer_id not in mdns users");
            bail!("peer_id not in mDNS users. Not dialing");
        };

        // only works if host has established connection via relay beforehand
        if !self.awaits_direct(&peer_id) {
            debug!(?peer_id, "peer not waiting for a direct connection");
            bail!("peer is not connected via relay only. Not dialing");
        }

        self.dial_peer(peer_id, &addr)
    }

    fn on_mdns(&mut self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(nodes) => {
                // Fortunately, this discovers all local multiaddr, so we need to prioritize
                // and try not to dial all of them ...
                debug!(?nodes, "mDNS discovered some nodes");
                let mut peer_map: BTreeMap<PeerId, Vec<Multiaddr>> = BTreeMap::new();

                for (peer, addr) in nodes {
                    peer_map.entry(peer).or_default().push(addr);
                }

                for addrs in peer_map.values_mut() {
                    addrs.sort_by_key(|addr| {
                        if addr.iter().any(|p| matches!(p, Protocol::Tcp(_))) {
                            0
                        } else if addr.iter().any(|p| matches!(p, Protocol::QuicV1)) {
                            1
                        } else {
                            2
                        }
                    });
                }

                for (peer, addrs) in peer_map {
                    for addr in addrs {
                        debug!(?peer, ?addr, "Handling node and peer");
                        self.mdns_users.insert(peer, addr);
                        // If the peer has not joined the room yet, the dial
                        // happens later from on_connection_established with
                        // whatever is in mdns_users — so the map must keep the
                        // preferred address, not the last candidate tried.
                        if !self.awaits_direct(&peer) {
                            break;
                        }
                        if self.dial_on_new_connection(peer).is_ok() {
                            // The resulting direct connection is reported to
                            // the core via on_connection_established.
                            break;
                        }
                    }
                }
            }
            mdns::Event::Expired(nodes) => {
                for node in nodes {
                    debug!(?node, "Nodes in mDNS expired");
                    self.mdns_users.remove(&node.0);
                }
            }
        }
    }

    fn on_connection_established(&mut self, peer_id: PeerId, endpoint: ConnectedPoint) {
        // The relay circuit is only a rendezvous for hole punching: messaging
        // runs on a dummy handler there (see DirectOnly). Track the peer so the
        // mDNS dialer can upgrade to a direct LAN connection, but do not report
        // it to the core until a direct connection exists.
        if endpoint.is_relayed() {
            debug!(%peer_id, "Client connected via relay; awaiting direct connection");
            if let Entry::Vacant(entry) = self.peers.entry(peer_id) {
                entry.insert(PeerLink::Relayed);
                if let Err(err) = self.dial_on_new_connection(peer_id) {
                    debug!(?err);
                }
            }
            return;
        }

        debug!(%peer_id, "Direct connection to client established");
        self.peers.insert(peer_id, PeerLink::Direct);

        self.handler
            .base
            .swarm
            .behaviour_mut()
            .file_share
            .inner_mut()
            .kademlia
            .add_address(&peer_id, endpoint.get_remote_address().clone());

        let gossip = &mut self
            .handler
            .base
            .swarm
            .behaviour_mut()
            .messaging
            .inner_mut()
            .gossipsub;
        gossip.add_explicit_peer(&peer_id);

        let res = self.handler.forward(PeerConnectedMsg { peer: peer_id });
        log_err_msg!(res, "Failed to report connected peer to core");
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        cause: Option<ConnectionError>,
        endpoint: ConnectedPoint,
    ) {
        if self.handler.base.swarm.is_connected(&peer_id) {
            return;
        }
        if peer_id == self.relay_peer_id {
            error!(
                ?endpoint,
                ?cause,
                "Connection of host to relay server closed"
            );
            self.handler.base.core_receiver.close();
            return;
        }

        debug!(%peer_id, "User connection stopped");
        // Gossipsub redials disconnected explicit peers forever; forget
        // departed clients.
        self.handler
            .base
            .swarm
            .behaviour_mut()
            .messaging
            .inner_mut()
            .gossipsub
            .remove_explicit_peer(&peer_id);
        if self.peers.remove(&peer_id).is_some() {
            let res = self.handler.forward(PeerDisconnectedMsg { peer: peer_id });
            log_err_msg!(res, "Failed to report disconnected peer to core");
        } else {
            warn!(?peer_id, "Expected peer to be known");
        }
    }

    fn on_core_handover(&mut self, request: HostHandoverRequest) -> Result<()> {
        self.handler
            .base
            .swarm
            .behaviour_mut()
            .transport
            .auth
            .transfer(
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
        // behaviours at this point. Closing the core receiver now would let the
        // run loop exit and drop the swarm before they are sent, so we keep the
        // loop alive until the relay answers the transfer request (see on_auth).
        self.handover_pending = true;
        Ok(())
    }

    fn on_auth(&mut self, event: AuthEvent) {
        if !self.handover_pending {
            return;
        }
        match event {
            AuthEvent::Complete { .. } => debug!("host handover acknowledged by relay"),
            AuthEvent::Failed(error) => error!(%error, "host handover failed"),
        }
        // The relay answered, so the transfer request (and the handover
        // broadcast queued before it) have left this node. Stop the host loop;
        // the core reconnects and rejoins as a regular client.
        self.handler.base.core_receiver.close();
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
                peer_id, endpoint, ..
            } => self.on_connection_established(peer_id, endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                ..
            } => self.on_connection_closed(peer_id, cause, endpoint),
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

    fn handle_swarm_broadcast(&mut self, data: Vec<u8>, source: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = data.try_into()?;
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
        let Err(msg) = self.handler.route_core_message(msg)? else {
            return Ok(());
        };
        use OutgoingMessage::*;
        match msg {
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
            UserStatus(m) => {
                warn!(?m, "The host's own status never leaves the host");
                Ok(())
            }
            other => bail!("Unrouted core message: {other:?}"),
        }
    }
}
