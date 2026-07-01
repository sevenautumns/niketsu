use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, gossipsub, mdns, ping};
use niketsu_core::communicator::{
    ConnectedMsg, PlaylistMsg, SeekMsg, SelectMsg, UserStatusMsg, VideoStatusMsg,
};
use tracing::{debug, error, info, warn};

use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait,
    DIRECT_CONNECT_TIMEOUT, FileShareBehaviourEvent, MessageResponse, SwarmHandler,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct ClientCommunicationHandler {
    handler: CommunicationHandler,
    /// Direct connection to the host, arriving either via DCUtR hole punch
    /// or via the host's mDNS LAN dial. Ping delay samples are only taken
    /// from this connection.
    host_conn: Option<ConnectionId>,
    /// Relayed (circuit) connections per peer: the rendezvous to the host and
    /// the file-share rendezvous to provider peers. A circuit only carries
    /// hole-punch coordination (see DirectOnly), so it is closed as soon as a
    /// direct connection to the same peer exists.
    relayed_conns: HashMap<PeerId, ConnectionId>,
    /// Whether we already listen on the host's relay circuit. Guards against
    /// creating a fresh relay-client listener (each one makes and renews its
    /// own reservation) every time the host connection is re-established.
    host_circuit_reserved: bool,
    video_status: VideoStatusMsg,
    is_seeking: bool,
    delay: Duration,
}

impl ClientCommunicationHandler {
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        let mut handler =
            CommunicationHandler::new(swarm, topic, host, core_receiver, message_sender);
        // We reach the client handler connected only via the relay circuit. Give
        // hole punching a bounded window to upgrade to a direct connection;
        // otherwise the loop aborts and the core reconnects.
        handler.base.arm_direct_deadline(DIRECT_CONNECT_TIMEOUT);
        Self {
            handler,
            host_conn: None,
            relayed_conns: HashMap::new(),
            host_circuit_reserved: false,
            video_status: VideoStatusMsg::default(),
            is_seeking: false,
            delay: Duration::default(),
        }
    }

    fn on_ping(&mut self, event: ping::Event) {
        debug!("Received ping!");
        if event.peer != self.handler.base.host {
            return;
        }

        match event.result {
            Ok(d) => {
                if let Some(conn) = self.host_conn
                    && event.connection == conn
                {
                    self.delay = d;
                }
            }
            Err(error) => {
                warn!(%error, "Ping to host failed, closing connection");
                self.handler.base.swarm.close_connection(event.connection);
            }
        }
    }

    /// Reserves a slot on the host's relay so other clients can reach us at
    /// `/p2p/HOST/p2p-circuit/p2p/SELF` as the rendezvous for file-share hole
    /// punches. Must be called from a one-shot spot after the direct host
    /// connection exists: the relay client only makes reservations over
    /// non-relayed connections, and every `listen_on` of the circuit spawns
    /// another listener whose own reservation renewals count against the host
    /// relay's per-peer rate limit.
    fn reserve_host_circuit(&mut self, host: PeerId) {
        if self.host_circuit_reserved {
            return;
        }
        let host_circuit = Multiaddr::empty()
            .with(Protocol::P2p(host))
            .with(Protocol::P2pCircuit);
        match self.handler.base.swarm.listen_on(host_circuit) {
            Ok(_) => {
                info!(%host, "Reserving slot on host relay");
                self.host_circuit_reserved = true;
            }
            Err(err) => warn!(%err, "Failed to listen on host relay circuit"),
        }
    }

    fn on_dcutr(&mut self, event: dcutr::Event) {
        let host = self.handler.base.host;
        match event.result {
            Ok(_) => {
                let gossip = &mut self
                    .handler
                    .base
                    .swarm
                    .behaviour_mut()
                    .messaging
                    .inner_mut()
                    .gossipsub;
                gossip.add_explicit_peer(&event.remote_peer_id);

                // The punched direct connection already triggered
                // on_connection_established, which closed the rendezvous
                // circuit and (for the host) cleared the deadline; this close
                // is only a fallback for event-ordering surprises.
                if let Some(conn) = self.relayed_conns.remove(&event.remote_peer_id) {
                    self.handler.base.swarm.close_connection(conn);
                }
                if event.remote_peer_id != host {
                    debug!(peer = %event.remote_peer_id, "Direct connection to non-host peer established");
                    return;
                }
                self.handler.base.clear_direct_deadline();
                info!("Established direct connection to host");
            }
            Err(error) => {
                error!(
                    %event.remote_peer_id, %error,
                    "Direct connection (hole punching) failed"
                );
                // A failed punch to another client (file sharing) is not fatal;
                // only a failed punch to the host means we can never become
                // connected, so hard-fail and let the core reconnect.
                if event.remote_peer_id == host {
                    warn!(%host, "Hole punching to host failed. Reconnecting");
                    self.handler.base.core_receiver.close();
                }
            }
        }
    }

    fn on_mdns(&mut self, event: mdns::Event) {
        if let mdns::Event::Discovered(list) = event {
            for (peer_id, addr) in list {
                self.handler
                    .base
                    .swarm
                    .behaviour_mut()
                    .file_share
                    .inner_mut()
                    .kademlia
                    .add_address(&peer_id, addr);
            }
        }
    }

    fn on_connection_established(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        // The relay circuit is only a rendezvous for hole punching: messaging
        // runs on a dummy handler there (see DirectOnly), so we do not mesh
        // gossipsub or report "connected" until a direct connection exists.
        if endpoint.is_relayed() {
            if peer_id == self.handler.base.host && self.host_conn.is_some() {
                // The host's mDNS LAN dial won the race against our circuit
                // dial; the rendezvous is already obsolete.
                debug!(%peer_id, "Closing relay circuit, direct connection already exists");
                self.handler.base.swarm.close_connection(connection_id);
            } else {
                self.relayed_conns.insert(peer_id, connection_id);
            }
            return;
        }

        // A direct connection makes the rendezvous circuit dead weight; close
        // it instead of letting it idle until the relay's circuit caps kill it
        // (and, worse, mask this peer's disconnect from is_connected checks).
        if let Some(conn) = self.relayed_conns.remove(&peer_id) {
            info!(%peer_id, "Direct connection established, closing relay circuit");
            self.handler.base.swarm.close_connection(conn);
        }

        if peer_id != self.handler.base.host {
            return;
        }

        info!(%connection_id, ?endpoint, "Direct connection to host established!");
        self.host_conn = Some(connection_id);
        self.reserve_host_circuit(peer_id);
        let gossip = &mut self
            .handler
            .base
            .swarm
            .behaviour_mut()
            .messaging
            .inner_mut()
            .gossipsub;
        gossip.add_explicit_peer(&peer_id);
        self.handler
            .base
            .swarm
            .behaviour_mut()
            .file_share
            .inner_mut()
            .kademlia
            .add_address(&peer_id, endpoint.get_remote_address().clone());
        self.handler.base.clear_direct_deadline();
        if let Err(error) = self
            .handler
            .base
            .message_sender
            .send(ConnectedMsg { is_host: false }.into())
        {
            warn!(%error, "Failed to send connected message to core");
        }
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        cause: Option<ConnectionError>,
        connection_id: ConnectionId,
    ) {
        if self.host_conn == Some(connection_id) {
            self.host_conn = None;
        }
        if self.relayed_conns.get(&peer_id) == Some(&connection_id) {
            self.relayed_conns.remove(&peer_id);
        }
        if self.handler.base.swarm.is_connected(&peer_id) {
            return;
        }
        if peer_id == self.handler.base.host {
            warn!(?cause, ?peer_id, host = %self.handler.base.host, %connection_id, "Connection to host closed");
            self.handler.base.core_receiver.close();
        } else {
            // Gossipsub redials disconnected explicit peers forever; forget
            // departed file-share peers added by on_dcutr.
            self.handler
                .base
                .swarm
                .behaviour_mut()
                .messaging
                .inner_mut()
                .gossipsub
                .remove_explicit_peer(&peer_id);
        }
    }

    fn on_outgoing_connection_error(
        &mut self,
        connection_id: ConnectionId,
        peer_id: Option<PeerId>,
        error: DialError,
    ) {
        let Some(pid) = peer_id else {
            warn!(%error, "Outgoing connection error with unknown peer");
            return;
        };

        if let Some(conn) = self.host_conn
            && connection_id != conn
        {
            warn!(%error, %connection_id, "Outgoing connection error with non-host. Ignoring");
            return;
        }

        if pid == self.handler.base.host && !self.handler.base.swarm.is_connected(&pid) {
            warn!(?error, ?peer_id, host = %self.handler.base.host, %connection_id, "Connection error to host");
            self.handler.base.core_receiver.close();
        }
    }

    fn on_broadcast_video_status(&mut self, msg: VideoStatusMsg, peer_id: PeerId) -> Result<()> {
        if peer_id != self.handler.base.host {
            anyhow::bail!("Received video status from non-host peer: {peer_id:?}")
        }

        if self.is_seeking {
            debug!("can not determine client position during seek");
            return Ok(());
        }

        let mut video_status = msg;
        if let Some(pos) = video_status.position
            && !video_status.paused
        {
            debug!("add delay to position");
            video_status.position = Some(pos + self.delay.div_f64(2.0));
        }

        self.handler.base.message_sender.send(video_status.into())?;
        Ok(())
    }

    fn on_broadcast_select(&mut self, msg: SelectMsg) -> Result<()> {
        self.handler.reset_requests_responses();
        self.handler.base.message_sender.send(msg.into())?;
        Ok(())
    }

    fn on_broadcast_seek(&mut self, msg: SeekMsg) -> Result<()> {
        self.is_seeking = true;
        self.handler.base.message_sender.send(msg.into())?;
        Ok(())
    }

    fn on_core_user_status(&mut self, msg: UserStatusMsg) -> Result<()> {
        let host = self.handler.base.host;
        self.handler.base.swarm.send_request(&host, msg.into());
        Ok(())
    }

    fn on_core_playlist(&mut self, msg: PlaylistMsg) -> Result<()> {
        let host = self.handler.base.host;
        self.handler.base.swarm.send_request(&host, msg.into());
        Ok(())
    }

    fn on_core_video_status(&mut self, msg: VideoStatusMsg) -> Result<()> {
        if msg.position != self.video_status.position {
            self.is_seeking = false;
            self.video_status = msg;
        }
        Ok(())
    }

    fn on_core_select(&mut self, msg: SelectMsg) -> Result<()> {
        let topic = self.handler.base.topic.clone();
        self.handler.reset_requests_responses();
        self.handler.base.swarm.try_broadcast(topic, msg.into())
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
        if peer_id == self.handler.base.host {
            self.handler.send_to_core(msg, channel)
        } else {
            self.handler.respond_with_err(msg, channel)
        }
    }

    fn handle_swarm_broadcast(&mut self, data: Vec<u8>, source: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = data.try_into()?;
        debug!(message = ?niketsu_msg, "Received broadcast");
        use NiketsuMessage::*;
        match niketsu_msg {
            VideoStatus(msg) => self.on_broadcast_video_status(msg, source),
            Select(msg) => self.on_broadcast_select(msg),
            Seek(msg) => self.on_broadcast_seek(msg),
            other => {
                self.handler.base.message_sender.send(other)?;
                Ok(())
            }
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

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(?msg, host = %self.handler.base.host, peer = %self.handler.base.swarm.local_peer_id(), "Handling core message");
        use NiketsuMessage::*;
        match msg {
            VideoStatus(m) => self.on_core_video_status(m),
            Select(m) => self.on_core_select(m),
            Playlist(m) => self.on_core_playlist(m),
            Status(m) => self.on_core_user_status(m),
            m @ (FileRequest(_) | FileResponse(_) | ChunkRequest(_) | ChunkResponse(_)
            | VideoShare(_)) => self.handler.handle_file_share_core_message(m),
            other => self.handler.broadcast(other),
        }
    }
}
