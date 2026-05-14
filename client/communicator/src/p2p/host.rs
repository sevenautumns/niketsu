use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, PlaylistMsg, SelectMsg, StartMsg, UserStatusListMsg, UserStatusMsg,
    VideoStatusMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::room::RoomName;
use niketsu_core::user::UserStatus;
use tracing::{debug, error, trace, warn};

use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, MessageResponse, MessagingBehaviourEvent, Response, StatusResponse,
    SwarmHandler,
};
use crate::messages::NiketsuMessage;
use crate::p2p::MessageRequest;

pub(crate) struct HostCommunicationHandler {
    handler: CommunicationHandler,
    relay_addr: Multiaddr,
    status_list: UserStatusListMsg,
    playlist: PlaylistMsg,
    select: SelectMsg,
    users: HashMap<PeerId, Option<UserStatus>>,
    mdns_users: HashMap<PeerId, Multiaddr>,
}

impl HostCommunicationHandler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_addr: Multiaddr,
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
        let handler = CommunicationHandler::new(
            swarm,
            topic,
            host,
            relay_addr.clone(),
            core_receiver,
            message_sender,
        );
        Self {
            handler,
            relay_addr,
            status_list: UserStatusListMsg {
                room_name: room,
                users: BTreeSet::default(),
            },
            playlist,
            users: HashMap::default(),
            select,
            mdns_users: HashMap::default(),
        }
    }

    fn send_init_status(&mut self, peer_id: PeerId) -> Result<()> {
        let status_list = self.status_list.clone();
        debug!(?status_list, "Sending initial status");

        let msg = NiketsuMessage::Playlist(self.playlist.clone());
        self.handler.swarm.send_request(&peer_id, msg);

        let msg = NiketsuMessage::Select(self.select.clone());
        self.handler.swarm.send_request(&peer_id, msg);

        let topic = self.handler.topic.clone();
        let msg = NiketsuMessage::StatusList(self.status_list.clone());
        self.handler.swarm.try_broadcast(topic, msg)
    }

    fn update_status(&mut self, status: UserStatus, peer_id: PeerId) {
        self.status_list.users.replace(status.clone());
        self.users.insert(peer_id, Some(status));
    }

    fn remove_peer(&mut self, status: &Option<UserStatus>, peer_id: &PeerId) {
        if let Some(s) = status {
            self.status_list.users.remove(s);
        }
        self.users.remove(peer_id);
    }

    //TODO: consider caching?
    fn all_users_ready(&self) -> bool {
        self.status_list.users.iter().all(|u| u.ready)
    }

    fn roll_new_username(&self, username: ArcStr) -> ArcStr {
        let mut buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        while self.username_exists(buzzword.clone()) {
            buzzword = arcstr::format!("{username}_{}", Buzzword().fake::<String>());
        }
        buzzword
    }

    // user that already sent status message and is known
    fn is_established_user(&self, peer_id: PeerId) -> bool {
        self.users.get(&peer_id).is_some_and(|s| s.is_some())
    }

    // user that established connection but did not sent status yet
    fn is_connected_user(&self, peer_id: PeerId) -> bool {
        self.users.get(&peer_id).is_some_and(|s| s.is_none())
    }

    fn username_exists(&self, name: ArcStr) -> bool {
        self.status_list.users.iter().any(|u| u.name == name)
    }

    fn handle_status(&mut self, status: UserStatus, peer_id: PeerId) {
        let mut new_status = status.clone();

        if self.is_established_user(peer_id) {
            // user name change, remove old status and update user map
            if !self.username_exists(status.name.clone()) {
                let users = self.users.clone(); // is there any other way to avoid mut/immut borrow?
                let old_status = users.get(&peer_id).expect("User should exist");
                self.remove_peer(old_status, &peer_id);
            }
            // otherwise, typical user status update, only need to update map & list
        } else if self.is_connected_user(peer_id) {
            // new user
            if self.username_exists(status.name.clone()) {
                // username needs to be force changed to avoid duplicate user names
                let new_username = self.roll_new_username(status.name.clone());
                new_status = UserStatus {
                    name: new_username,
                    ready: status.ready,
                };
                let msg = NiketsuMessage::Status(new_status.clone());
                self.handler.swarm.send_request(&peer_id, msg);
            }
        }

        self.update_status(new_status, peer_id);
    }

    fn handle_all_users_ready(&mut self, peer_id: PeerId) -> Result<()> {
        if self.all_users_ready() {
            debug!("All users area ready. Publishing start to gossipsub");
            let mut start_msg = NiketsuMessage::Start(StartMsg {
                actor: arcstr::literal!("server"),
            });
            if let Some(status) = self.users.get(&peer_id) {
                let actor = match status {
                    Some(s) => s.name.clone(),
                    None => arcstr::literal!("unknown"),
                };

                start_msg = NiketsuMessage::Start(StartMsg { actor });
            }
            self.handler.message_sender.send(start_msg.clone())?;
            let topic = self.handler.topic.clone();
            self.handler.swarm.try_broadcast(topic, start_msg)?;
        }
        Ok(())
    }

    fn select_next(&mut self, new_playlist: &PlaylistMsg) -> Option<SelectMsg> {
        if new_playlist.playlist.len() >= self.playlist.playlist.len() {
            return None;
        }

        let mut new_position = 0;
        let max_len = new_playlist.playlist.len();
        if let Some(current_video) = self.select.video.clone() {
            for old_video in self.playlist.playlist.iter() {
                if let Some(new_video) = new_playlist.playlist.get(new_position) {
                    debug!(?current_video, ?new_video, "Current video and old");
                    if *new_video == current_video {
                        // No need to select if current video is still in playlist
                        return None;
                    }

                    if *old_video == current_video {
                        break;
                    }

                    if *new_video == *old_video {
                        new_position += 1;
                    }

                    if new_position >= max_len {
                        new_position -= 1;
                        break;
                    }
                }
            }
        }

        let new_select = new_playlist.playlist.get(new_position)?;
        Some(SelectMsg {
            actor: arcstr::format!("host"),
            position: Duration::ZERO,
            video: Some(new_select.clone()),
        })
    }

    fn handle_new_playlist(&mut self, playlist: &PlaylistMsg, peer_id: PeerId) -> Result<()> {
        if let Some(select_msg) = self.select_next(playlist) {
            self.select = select_msg.clone();
            let msg: NiketsuMessage = select_msg.into();
            self.handler.message_sender.send(msg.clone())?;
            let topic = self.handler.topic.clone();
            self.handler.swarm.try_broadcast(topic, msg)?;
            self.handle_all_users_ready(peer_id)?;
        }
        Ok(())
    }

    fn dial_peer(&mut self, peer_id: PeerId, addr: &Multiaddr) -> Result<()> {
        if let Err(err) = self.handler.swarm.dial(addr.clone()) {
            warn!(?peer_id, ?err, "Failed to dial mDNS node");
            bail!("Failed to dial mDNS node");
        } else {
            debug!(?peer_id, "Dialing mDNS node");

            let kad = &mut self.handler.swarm.behaviour_mut().file_share.kademlia;
            kad.add_address(&peer_id, addr.clone());

            let gossip = &mut self.handler.swarm.behaviour_mut().messaging.gossipsub;
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
        if !self.is_connected_user(peer_id) {
            debug!(?peer_id, "peer not connected via relay");
            bail!("peer is not connected via relay. Not dialing");
        }

        self.dial_peer(peer_id, addr)
    }

    fn on_gossipsub(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => {
                debug!(%message_id, %message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                let res = self.handle_swarm_broadcast(message.data, propagation_source);
                log_err_msg!(res, "Failed to handle broadcast message")
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }

    fn on_msg_req_resp(&mut self, event: request_response::Event<MessageRequest, MessageResponse>) {
        match event {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = self.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message")
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    let res = self.handler.handle_swarm_response(response, peer);
                    log_err_msg!(res, "Failed to handle incoming message")
                }
            },
            request_response_event => debug!(
                ?request_response_event,
                "Received request response event that is not handled"
            ),
        }
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
                        // put into map until relay connection established
                        // make sure not to overload dialing all multiaddr
                        self.mdns_users.insert(peer, addr);
                        if self.dial_on_new_connection(peer).is_ok() {
                            let res = self.send_init_status(peer);
                            log_err_msg!(res, "Failed to send initial messages to client");
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
        debug!(%peer_id, "New client established connection");

        // Skip if already connected to avoid spam (mDNS dial triggers new connections)
        if self.users.contains_key(&peer_id) {
            return;
        }

        self.users.insert(peer_id, None);
        if let Err(err) = self.dial_on_new_connection(peer_id) {
            debug!(?err);
        }

        self.handler
            .swarm
            .behaviour_mut()
            .transport
            .relay_server
            .allow(peer_id);

        self.handler
            .swarm
            .behaviour_mut()
            .file_share
            .kademlia
            .add_address(&peer_id, endpoint.get_remote_address().clone());

        let res = self.send_init_status(peer_id);
        log_err_msg!(res, "Failed to send initial messages to client");
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        cause: Option<ConnectionError>,
        endpoint: ConnectedPoint,
    ) {
        if self.relay_addr == (*endpoint.get_remote_address()) {
            error!(
                ?endpoint,
                ?cause,
                "Connection of host to relay server closed"
            );
            self.handler.core_receiver.close();
        } else if !self.handler.swarm.is_connected(&peer_id) {
            debug!("User connection stopped and user removed from map");
            self.handler
                .swarm
                .behaviour_mut()
                .transport
                .relay_server
                .deny(&peer_id);
            let users = self.users.clone();
            let topic = self.handler.topic.clone();
            if let Some(status) = users.get(&peer_id) {
                self.remove_peer(status, &peer_id);
                let status_list = NiketsuMessage::StatusList(self.status_list.clone());
                let res = self.handler.message_sender.send(status_list.clone());
                log_err_msg!(res, "Failed to send status list to core");
                let res = self.handler.swarm.try_broadcast(topic, status_list);
                log_err_msg!(res, "Failed to broadcast status list");
            } else {
                warn!(?peer_id, "Expected peer to be included in list");
            }
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

    fn on_swarm_request_user_status(
        &mut self,
        msg: UserStatusMsg,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        self.handle_status(msg, peer_id);
        if let Err(err) = self.handle_all_users_ready(peer_id) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler.swarm.send_message_response(channel, resp)?;
            return Err(err);
        }

        let msg = NiketsuMessage::StatusList(self.status_list.clone());
        if let Err(err) = self.handler.message_sender.send(msg.clone()) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler.swarm.send_message_response(channel, resp)?;
            return Err(anyhow::Error::from(err));
        }

        let topic = self.handler.topic.clone();
        let resp = match self.handler.swarm.try_broadcast(topic, msg) {
            Ok(_) => MessageResponse(Response::Status(StatusResponse::Ok)),
            Err(_) => MessageResponse(Response::Status(StatusResponse::Err)),
        };
        self.handler.swarm.send_message_response(channel, resp)
    }

    fn on_swarm_request_playlist(
        &mut self,
        msg: PlaylistMsg,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let wrapped = NiketsuMessage::Playlist(msg.clone());
        if let Err(err) = self.handler.message_sender.send(wrapped.clone()) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler.swarm.send_message_response(channel, resp)?;
            return Err(anyhow::Error::from(err));
        }

        let topic = self.handler.topic.clone();
        if let Err(err) = self.handler.swarm.try_broadcast(topic, wrapped) {
            let resp = MessageResponse(Response::Status(StatusResponse::Err));
            self.handler.swarm.send_message_response(channel, resp)?;
            return Err(err);
        }

        match self.handle_new_playlist(&msg, peer_id) {
            Ok(_) => {
                self.playlist = msg;
                let resp = MessageResponse(Response::Status(StatusResponse::Ok));
                self.handler.swarm.send_message_response(channel, resp)
            }
            Err(_) => self.handler.swarm.send_message_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
    }

    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        debug!(message = ?niketsu_msg, "Handling broadcast message from swarm");
        use NiketsuMessage::*;
        match niketsu_msg {
            Select(m) => self.on_broadcast_select(m, peer_id),
            m @ (Pause(_) | Start(_) | PlaybackSpeed(_) | Seek(_) | UserMessage(_)) => {
                self.handler.message_sender.send(m)?;
                Ok(())
            }
            other => bail!("Host received unexpected broadcast message: {other:?}"),
        }
    }

    fn on_broadcast_select(&mut self, msg: SelectMsg, peer_id: PeerId) -> Result<()> {
        let wrapped = NiketsuMessage::Select(msg.clone());
        self.select = msg;
        self.handler.message_sender.send(wrapped)?;
        self.handle_all_users_ready(peer_id)?;
        self.handler.reset_requests_responses();
        Ok(())
    }

    fn on_core_user_status(&mut self, msg: UserStatusMsg) -> Result<()> {
        let peer_id = self.handler.host;
        self.update_status(msg, peer_id);
        self.handle_all_users_ready(peer_id)?;
        let niketsu_msg = NiketsuMessage::StatusList(self.status_list.clone());
        self.handler.message_sender.send(niketsu_msg.clone())?; // is this necessary?
        let topic = self.handler.topic.clone();
        self.handler.swarm.try_broadcast(topic, niketsu_msg)
    }

    fn on_core_playlist(&mut self, msg: PlaylistMsg) -> Result<()> {
        self.handle_new_playlist(&msg, self.handler.host)?;
        self.playlist = msg.clone();
        let topic = self.handler.topic.clone();
        self.handler.swarm.try_broadcast(topic, msg.into())
    }

    fn on_core_video_status(&mut self, msg: VideoStatusMsg) -> Result<()> {
        self.select.position = msg.position.unwrap_or_default();
        let topic = self.handler.topic.clone();
        self.handler.swarm.try_broadcast(topic, msg.into())
    }

    fn on_core_select(&mut self, msg: SelectMsg) -> Result<()> {
        self.select = msg.clone();
        let topic = self.handler.topic.clone();
        self.handler.swarm.try_broadcast(topic, msg.into())?;
        self.handle_all_users_ready(self.handler.host)?;
        self.handler.reset_requests_responses();
        Ok(())
    }
}

#[async_trait]
impl CommunicationHandlerTrait for HostCommunicationHandler {
    async fn run(&mut self) {
        if let Err(error) = self.handler.message_sender.send(ConnectedMsg.into()) {
            warn!(%error, "Failed to send connected message to core");
        }

        loop {
            let base = &mut self.handler.base;
            tokio::select! {
                event = base.swarm.select_next_some() => self.handle_swarm_event(event),
                msg = base.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "core message");
                        let res = self.handle_core_message(msg);
                        log_err_msg!(res, "Handling message caused error");
                    },
                    None => {
                        error!("Channel of core closed. Stopping p2p host event loop");
                        break
                    },
                },
            }
        }
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        debug!(?event, "Handling event from swarm");
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        use MessagingBehaviourEvent as M;
        match event {
            SwarmEvent::Behaviour(Messaging(M::Gossipsub(e))) => self.on_gossipsub(e),
            SwarmEvent::Behaviour(Messaging(M::RequestResponse(e))) => self.on_msg_req_resp(e),
            SwarmEvent::Behaviour(FileShare(F::RequestResponse(e))) => {
                self.handler.handle_file_share_req_resp_event(e)
            }
            SwarmEvent::Behaviour(FileShare(F::Kademlia(e))) => {
                self.handler.handle_file_share_kad_event(e)
            }
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

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(host = %self.handler.host, ?msg, "Handling core message");
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
