use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::{ConnectionError, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostHandoverMsg, PlaylistMsg, SelectMsg, StartMsg, UserStatusMsg, VideoStatusMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::room::RoomName;
use niketsu_core::user::UserStatus;
use tracing::{debug, error, warn};

use super::auth::AuthEvent;
use super::room::{RoomUsers, select_next};
use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, MessageResponse, Response, StatusResponse, SwarmHandler,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct HostCommunicationHandler {
    handler: CommunicationHandler,
    relay_peer_id: PeerId,
    password: String,
    room: RoomUsers,
    playlist: PlaylistMsg,
    select: SelectMsg,
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
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
        room: RoomName,
        playlist_handler: PlaylistHandler,
    ) -> Self {
        let playlist = PlaylistMsg {
            actor: arcstr::literal!("host"),
            playlist: playlist_handler.get_playlist(),
        };
        let select = SelectMsg {
            actor: arcstr::literal!("host"),
            position: Duration::default(),
            video: playlist_handler.get_current_video(),
        };
        message_sender.send(playlist.clone().into()).ok();
        message_sender.send(select.clone().into()).ok();
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let handler = CommunicationHandler::new(swarm, topic, host, core_receiver, message_sender);
        Self {
            handler,
            relay_peer_id,
            password,
            room: RoomUsers::new(room),
            playlist,
            select,
            mdns_users: HashMap::default(),
            handover_pending: false,
        }
    }

    fn send_init_status(&mut self, peer_id: PeerId) -> Result<()> {
        let status_list = self.room.status_list().clone();
        debug!(?status_list, "Sending initial status");

        let msg = NiketsuMessage::Playlist(self.playlist.clone());
        self.handler.base.swarm.send_request(&peer_id, msg);

        let msg = NiketsuMessage::Select(self.select.clone());
        self.handler.base.swarm.send_request(&peer_id, msg);

        let topic = self.handler.base.topic.clone();
        let msg = NiketsuMessage::StatusList(status_list);
        self.handler.base.swarm.try_broadcast(topic, msg)
    }

    fn handle_status(&mut self, status: UserStatus, peer_id: PeerId) {
        if let Some(forced) = self.room.apply_status(status, peer_id) {
            let msg = NiketsuMessage::Status(forced);
            self.handler.base.swarm.send_request(&peer_id, msg);
        }
    }

    fn handle_all_users_ready(&mut self, peer_id: PeerId) -> Result<()> {
        if self.room.all_ready() {
            debug!("All users are ready. Publishing start to gossipsub");
            let actor = match self.room.peer_status(&peer_id) {
                Some(Some(status)) => status.name.clone(),
                Some(None) => arcstr::literal!("unknown"),
                None => arcstr::literal!("server"),
            };
            let start_msg = NiketsuMessage::Start(StartMsg { actor });
            self.handler.base.message_sender.send(start_msg.clone())?;
            let topic = self.handler.base.topic.clone();
            self.handler.base.swarm.try_broadcast(topic, start_msg)?;
        }
        Ok(())
    }

    fn handle_new_playlist(&mut self, playlist: &PlaylistMsg, peer_id: PeerId) -> Result<()> {
        if let Some(select_msg) = select_next(&self.playlist, &self.select, playlist) {
            self.select = select_msg.clone();
            let msg: NiketsuMessage = select_msg.into();
            self.handler.base.message_sender.send(msg.clone())?;
            let topic = self.handler.base.topic.clone();
            self.handler.base.swarm.try_broadcast(topic, msg)?;
            self.handle_all_users_ready(peer_id)?;
        }
        Ok(())
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
        let mdns_users = self.mdns_users.clone();
        let Some(addr) = mdns_users.get(&peer_id) else {
            debug!(?peer_id, "peer_id not in mdns users");
            bail!("peer_id not in mDNS users. Not dialing");
        };

        // only works if host has established connection via relay beforehand
        if !self.room.is_connected_user(peer_id) {
            debug!(?peer_id, "peer not connected via relay");
            bail!("peer is not connected via relay. Not dialing");
        }

        self.dial_peer(peer_id, addr)
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
                        // happens later from on_room_auth with whatever is in
                        // mdns_users — so the map must keep the preferred
                        // address, not the last candidate tried.
                        if !self.room.is_connected_user(peer) {
                            break;
                        }
                        if self.dial_on_new_connection(peer).is_ok() {
                            // The resulting direct connection sends the initial
                            // room state via on_connection_established.
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
        // mDNS dialer can upgrade to a direct LAN connection, but do not send
        // room state until a direct connection exists.
        if endpoint.is_relayed() {
            debug!(%peer_id, "Client connected via relay; awaiting direct connection");
            if !self.room.contains(&peer_id) {
                self.room.add_connected(peer_id);
                if let Err(err) = self.dial_on_new_connection(peer_id) {
                    debug!(?err);
                }
            }
            return;
        }

        debug!(%peer_id, "Direct connection to client established");
        if !self.room.contains(&peer_id) {
            self.room.add_connected(peer_id);
        }

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

        let res = self.send_init_status(peer_id);
        log_err_msg!(res, "Failed to send initial messages to client");
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
        } else {
            debug!("User connection stopped and user removed from map");
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
            let topic = self.handler.base.topic.clone();
            if self.room.remove(&peer_id) {
                let status_list = NiketsuMessage::StatusList(self.room.status_list().clone());
                let res = self.handler.base.message_sender.send(status_list.clone());
                log_err_msg!(res, "Failed to send status list to core");
                let res = self.handler.base.swarm.try_broadcast(topic, status_list);
                log_err_msg!(res, "Failed to broadcast status list");
            } else {
                warn!(?peer_id, "Expected peer to be included in list");
            }
        }
    }

    fn on_swarm_request_user_status(
        &mut self,
        msg: UserStatusMsg,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        self.handle_status(msg, peer_id);
        if let Err(err) = self.handle_all_users_ready(peer_id) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler
                .base
                .swarm
                .send_message_response(channel, resp)?;
            return Err(err);
        }

        let msg = NiketsuMessage::StatusList(self.room.status_list().clone());
        if let Err(err) = self.handler.base.message_sender.send(msg.clone()) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler
                .base
                .swarm
                .send_message_response(channel, resp)?;
            return Err(anyhow::Error::from(err));
        }

        let topic = self.handler.base.topic.clone();
        let resp = match self.handler.base.swarm.try_broadcast(topic, msg) {
            Ok(_) => MessageResponse(Response::Status(StatusResponse::Ok)),
            Err(_) => MessageResponse(Response::Status(StatusResponse::Err)),
        };
        self.handler.base.swarm.send_message_response(channel, resp)
    }

    fn on_swarm_request_playlist(
        &mut self,
        msg: PlaylistMsg,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let wrapped = NiketsuMessage::Playlist(msg.clone());
        if let Err(err) = self.handler.base.message_sender.send(wrapped.clone()) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler
                .base
                .swarm
                .send_message_response(channel, resp)?;
            return Err(anyhow::Error::from(err));
        }

        let topic = self.handler.base.topic.clone();
        if let Err(err) = self.handler.base.swarm.try_broadcast(topic, wrapped) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler
                .base
                .swarm
                .send_message_response(channel, resp)?;
            return Err(err);
        }

        match self.handle_new_playlist(&msg, peer_id) {
            Ok(_) => {
                self.playlist = msg;
                let resp = MessageResponse(Response::Status(StatusResponse::Ok));
                self.handler.base.swarm.send_message_response(channel, resp)
            }
            Err(_) => self.handler.base.swarm.send_message_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
    }

    fn on_core_handover(&mut self, msg: HostHandoverMsg) -> Result<()> {
        let Some(peer_id) = self.room.find_by_name(&msg.new_host) else {
            bail!("handover target '{}' not found in user list", msg.new_host);
        };

        self.handler
            .base
            .swarm
            .behaviour_mut()
            .transport
            .auth
            .transfer(
                self.relay_peer_id,
                self.room.status_list().room_name.clone(),
                self.password.clone(),
                peer_id,
            );

        let topic = self.handler.base.topic.clone();
        self.handler
            .base
            .swarm
            .try_broadcast(topic, NiketsuMessage::HostHandover(msg))?;

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

    fn on_broadcast_select(&mut self, msg: SelectMsg, peer_id: PeerId) -> Result<()> {
        let wrapped = NiketsuMessage::Select(msg.clone());
        self.select = msg;
        self.handler.base.message_sender.send(wrapped)?;
        self.handle_all_users_ready(peer_id)?;
        self.handler.reset_requests_responses();
        Ok(())
    }

    fn on_core_user_status(&mut self, msg: UserStatusMsg) -> Result<()> {
        let peer_id = self.handler.base.host;
        self.room.update_status(msg, peer_id);
        self.handle_all_users_ready(peer_id)?;
        let niketsu_msg = NiketsuMessage::StatusList(self.room.status_list().clone());
        self.handler.base.message_sender.send(niketsu_msg.clone())?; // is this necessary?
        let topic = self.handler.base.topic.clone();
        self.handler.base.swarm.try_broadcast(topic, niketsu_msg)
    }

    fn on_core_playlist(&mut self, msg: PlaylistMsg) -> Result<()> {
        self.handle_new_playlist(&msg, self.handler.base.host)?;
        self.playlist = msg.clone();
        let topic = self.handler.base.topic.clone();
        self.handler.base.swarm.try_broadcast(topic, msg.into())
    }

    fn on_core_video_status(&mut self, msg: VideoStatusMsg) -> Result<()> {
        self.select.position = msg.position.unwrap_or_default();
        let topic = self.handler.base.topic.clone();
        self.handler.base.swarm.try_broadcast(topic, msg.into())
    }

    fn on_core_select(&mut self, msg: SelectMsg) -> Result<()> {
        self.select = msg.clone();
        let topic = self.handler.base.topic.clone();
        self.handler.base.swarm.try_broadcast(topic, msg.into())?;
        self.handle_all_users_ready(self.handler.base.host)?;
        self.handler.reset_requests_responses();
        Ok(())
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
            Playlist(m) => self.on_swarm_request_playlist(m, peer_id, channel),
            Status(m) => self.on_swarm_request_user_status(m, peer_id, channel),
            other => self.handler.respond_with_err(other, channel),
        }
    }

    fn handle_swarm_broadcast(&mut self, data: Vec<u8>, source: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = data.try_into()?;
        debug!(message = ?niketsu_msg, "Handling broadcast message from swarm");
        use NiketsuMessage::*;
        match niketsu_msg {
            Select(m) => self.on_broadcast_select(m, source),
            m @ (Pause(_) | Start(_) | PlaybackSpeed(_) | Seek(_) | UserMessage(_)) => {
                self.handler.base.message_sender.send(m)?;
                Ok(())
            }
            other => bail!("Host received unexpected broadcast message: {other:?}"),
        }
    }

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(host = %self.handler.base.host, ?msg, "Handling core message");
        use NiketsuMessage::*;
        match msg {
            VideoStatus(m) => self.on_core_video_status(m),
            Select(m) => self.on_core_select(m),
            Playlist(m) => self.on_core_playlist(m),
            Status(m) => self.on_core_user_status(m),
            HostHandover(m) => self.on_core_handover(m),
            m @ (FileRequest(_) | FileResponse(_) | ChunkRequest(_) | ChunkResponse(_)
            | VideoShare(_)) => self.handler.handle_file_share_core_message(m),
            other => self.handler.broadcast(other),
        }
    }
}
