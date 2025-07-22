use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::kad::{self};
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

use super::file_share::FileShareEventHandler;
use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait, MessageResponse,
    Response, StatusResponse, SwarmHandler,
};
use crate::messages::NiketsuMessage;
use crate::p2p::MessageRequest;
use crate::p2p::file_share::{FileShareCoreMessageHandler, FileShareSwarmRequestHandler};

#[enum_dispatch]
pub(crate) trait HostSwarmEventHandler {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler);
}

#[enum_dispatch(HostSwarmEventHandler)]
enum HostSwarmEvent {
    GossipSub(gossipsub::Event),
    MessageRequestResponse(request_response::Event<MessageRequest, MessageResponse>),
    Kademlia(kad::Event),
    Mdns(mdns::Event),
    ConnectionEstablished(ConnectionEstablished),
    ConnectionClosed(ConnectionClosed),
    Other(Box<SwarmEvent<BehaviourEvent>>),
}

impl HostSwarmEvent {
    fn from(event: SwarmEvent<BehaviourEvent>) -> Self {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(event)) => {
                HostSwarmEvent::GossipSub(event)
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(event)) => {
                HostSwarmEvent::MessageRequestResponse(event)
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => {
                HostSwarmEvent::Kademlia(event)
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                endpoint,
                cause,
                ..
            } => HostSwarmEvent::ConnectionClosed(ConnectionClosed {
                peer_id,
                cause,
                endpoint,
            }),
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => HostSwarmEvent::ConnectionEstablished(ConnectionEstablished { peer_id, endpoint }),
            SwarmEvent::Behaviour(BehaviourEvent::Mdns(event)) => HostSwarmEvent::Mdns(event),
            _ => HostSwarmEvent::Other(Box::new(event)),
        }
    }
}

impl HostSwarmEventHandler for gossipsub::Event {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        match self {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => {
                debug!(%message_id, %message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                let res = handler.handle_swarm_broadcast(message.data, propagation_source);
                log_err_msg!(res, "Failed to handle broadcast message")
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }
}

impl HostSwarmEventHandler for request_response::Event<MessageRequest, MessageResponse> {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        match self {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = handler.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message")
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    let res = handler.handle_swarm_response(response, peer);
                    log_err_msg!(res, "Failed to handle incoming message")
                }
            },
            request_response_event => debug!(
                ?request_response_event,
                "Received request response event that is not handled"
            ),
        }
    }
}

impl HostSwarmEventHandler for kad::Event {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        FileShareEventHandler::handle_event(self, &mut handler.handler);
    }
}

impl HostSwarmEventHandler for mdns::Event {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        match self {
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
                        handler.mdns_users.insert(peer, addr);
                        if handler.dial_on_new_connection(peer).is_ok() {
                            let res = handler.send_init_status(peer);
                            log_err_msg!(res, "Failed to send initial messages to client");
                            break;
                        }
                    }
                }
            }
            mdns::Event::Expired(nodes) => {
                for node in nodes {
                    debug!(?node, "Nodes in mDNS expired");
                    handler.mdns_users.remove(&node.0);
                }
            }
        }
    }
}

struct ConnectionEstablished {
    peer_id: PeerId,
    endpoint: ConnectedPoint,
}

impl HostSwarmEventHandler for ConnectionEstablished {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        debug!(%self.peer_id, "New client established connection");

        // Skip if already connected to avoid spam (mDNS dial triggers new connections)
        if handler.users.contains_key(&self.peer_id) {
            return;
        }

        handler.users.insert(self.peer_id, None);
        if let Err(err) = handler.dial_on_new_connection(self.peer_id) {
            debug!(?err);
        }

        handler
            .handler
            .swarm
            .behaviour_mut()
            .kademlia
            .add_address(&self.peer_id, self.endpoint.get_remote_address().clone());

        let res = handler.send_init_status(self.peer_id);
        log_err_msg!(res, "Failed to send initial messages to client");
    }
}

struct ConnectionClosed {
    peer_id: PeerId,
    cause: Option<ConnectionError>,
    endpoint: ConnectedPoint,
}

impl HostSwarmEventHandler for ConnectionClosed {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        if handler.relay_addr == (*self.endpoint.get_remote_address()) {
            error!(?self.endpoint, ?self.cause, "Connection of host to relay server closed");
            handler.handler.core_receiver.close();
        } else if !handler.handler.swarm.is_connected(&self.peer_id) {
            debug!("User connection stopped and user removed from map");
            let users = handler.users.clone();
            let topic = handler.handler.topic.clone();
            if let Some(status) = users.get(&self.peer_id) {
                handler.remove_peer(status, &self.peer_id);
                let status_list = NiketsuMessage::StatusList(handler.status_list.clone());
                let res = handler.handler.message_sender.send(status_list.clone());
                log_err_msg!(res, "Failed to send status list to core");
                let res = handler.handler.swarm.try_broadcast(topic, status_list);
                log_err_msg!(res, "Failed to broadcast status list");
            } else {
                warn!(?self.peer_id, "Expected peer to be included in list");
            }
        }
    }
}

impl HostSwarmEventHandler for Box<SwarmEvent<BehaviourEvent>> {
    fn handle_swarm_event(self, _handler: &mut HostCommunicationHandler) {
        debug!(event = ?self, "Received not captured event")
    }
}

#[enum_dispatch]
pub(crate) trait HostCoreMessageHandler {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()>;
}

impl HostCoreMessageHandler for UserStatusMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        let peer_id = handler.handler.host;
        handler.update_status(self, peer_id);
        handler.handle_all_users_ready(peer_id)?;
        let niketsu_msg = NiketsuMessage::StatusList(handler.status_list.clone());
        handler.handler.message_sender.send(niketsu_msg.clone())?; // is this necessary?
        let topic = handler.handler.topic.clone();
        handler.handler.swarm.try_broadcast(topic, niketsu_msg)
    }
}

impl HostCoreMessageHandler for PlaylistMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.handle_new_playlist(&self, handler.handler.host)?;
        handler.playlist = self.clone();
        let topic = handler.handler.topic.clone();
        handler.handler.swarm.try_broadcast(topic, self.into())
    }
}

impl HostCoreMessageHandler for VideoStatusMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.select.position = self.position.unwrap_or_default();
        let topic = handler.handler.topic.clone();
        handler.handler.swarm.try_broadcast(topic, self.into())
    }
}

impl HostCoreMessageHandler for SelectMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.select = self.clone();
        let topic = handler.handler.topic.clone();
        handler.handler.swarm.try_broadcast(topic, self.into())?;
        handler.handle_all_users_ready(handler.handler.host)?;
        handler.handler.reset_requests_responses();
        Ok(())
    }
}

#[enum_dispatch]
pub(crate) trait HostSwarmRequestHandler {
    fn handle_swarm_request(
        self,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()>;
}

impl HostSwarmRequestHandler for UserStatusMsg {
    fn handle_swarm_request(
        self,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        handler.handle_status(self.clone(), peer_id);
        if let Err(err) = handler.handle_all_users_ready(peer_id) {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(err);
        }

        let msg = NiketsuMessage::StatusList(handler.status_list.clone());
        if let Err(err) = handler.handler.message_sender.send(msg.clone()) {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(anyhow::Error::from(err));
        }

        let topic = handler.handler.topic.clone();
        match handler.handler.swarm.try_broadcast(topic, msg) {
            Ok(_) => handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Ok)),
            ),
            Err(_) => handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
    }
}

impl HostSwarmRequestHandler for PlaylistMsg {
    fn handle_swarm_request(
        self,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        let msg = NiketsuMessage::Playlist(self.clone());
        if let Err(err) = handler.handler.message_sender.send(msg.clone()) {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(anyhow::Error::from(err));
        }

        let topic = handler.handler.topic.clone();
        if let Err(err) = handler.handler.swarm.try_broadcast(topic, msg) {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(err);
        }

        match handler.handle_new_playlist(&self, peer_id) {
            Ok(_) => {
                handler.playlist = self.clone();
                handler.handler.swarm.send_response(
                    channel,
                    MessageResponse(Response::Status(StatusResponse::Ok)),
                )
            }
            Err(_) => handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
    }
}

#[enum_dispatch()]
trait HostSwarmBroadcastHandler {
    fn handle_swarm_broadcast(
        self,
        peer_id: PeerId,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()>;
}

#[enum_dispatch(HostSwarmBroadcastHandler)]
enum HostSwarmBroadcast {
    Select(SelectMsg),
    Passthrough(PassthroughMsg),
    Other(NiketsuMessage),
}

impl HostSwarmBroadcast {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::Select(msg) => HostSwarmBroadcast::Select(msg),
            NiketsuMessage::Pause(_)
            | NiketsuMessage::Start(_)
            | NiketsuMessage::PlaybackSpeed(_)
            | NiketsuMessage::Seek(_)
            | NiketsuMessage::UserMessage(_) => HostSwarmBroadcast::Passthrough(PassthroughMsg {
                niketsu_msg: message,
            }),
            msg => HostSwarmBroadcast::Other(msg),
        }
    }
}

#[derive(Debug)]
struct PassthroughMsg {
    niketsu_msg: NiketsuMessage,
}

impl HostSwarmBroadcastHandler for SelectMsg {
    fn handle_swarm_broadcast(
        self,
        peer_id: PeerId,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        let msg = NiketsuMessage::Select(self.clone());
        handler.select = self;
        handler.handler.message_sender.send(msg)?;
        handler.handle_all_users_ready(peer_id)?;
        handler.handler.reset_requests_responses();
        Ok(())
    }
}

impl HostSwarmBroadcastHandler for PassthroughMsg {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        handler
            .handler
            .message_sender
            .send(self.niketsu_msg)
            .map_err(anyhow::Error::from)
    }
}

impl HostSwarmBroadcastHandler for NiketsuMessage {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        _handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        bail!("Host received unexpected broadcast message: {self:?}")
    }
}

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
                self.handler
                    .swarm
                    .send_request(&peer_id, NiketsuMessage::Status(new_status.clone()));
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

        new_playlist
            .playlist
            .get(new_position)
            .map(|new_select| SelectMsg {
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

            self.handler
                .swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr.clone());

            self.handler
                .swarm
                .behaviour_mut()
                .gossipsub
                .add_explicit_peer(&peer_id);
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
        let host_event = HostSwarmEvent::from(event);
        host_event.handle_swarm_event(self);
    }

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(host = %self.handler.host, ?msg, "Handling core message");
        use FileShareCoreMessageHandler as FH;
        use NiketsuMessage::*;
        match msg {
            VideoStatus(msg) => HostCoreMessageHandler::handle_core_message(msg, self),
            Select(msg) => HostCoreMessageHandler::handle_core_message(msg, self),
            Playlist(msg) => HostCoreMessageHandler::handle_core_message(msg, self),
            Status(msg) => HostCoreMessageHandler::handle_core_message(msg, self),
            FileRequest(msg) => FH::handle_core_message(msg, &mut self.handler),
            FileResponse(msg) => FH::handle_core_message(msg, &mut self.handler),
            ChunkRequest(msg) => FH::handle_core_message(msg, &mut self.handler),
            ChunkResponse(msg) => FH::handle_core_message(msg, &mut self.handler),
            VideoShare(msg) => FH::handle_core_message(msg, &mut self.handler),
            msg => msg.broadcast(&mut self.handler),
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
        use {FileShareSwarmRequestHandler as FH, HostSwarmRequestHandler as SH};
        match msg {
            Playlist(msg) => SH::handle_swarm_request(msg, peer_id, channel, self),
            Status(msg) => SH::handle_swarm_request(msg, peer_id, channel, self),
            FileRequest(msg) => FH::handle_swarm_request(msg, channel, &mut self.handler),
            ChunkRequest(msg) => FH::handle_swarm_request(msg, channel, &mut self.handler),
            msg => msg.respond_with_err(channel, &mut self.handler),
        }
    }

    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        debug!(message = ?niketsu_msg, "Handling broadcast message from swarm");
        let swarm_broadcast = HostSwarmBroadcast::from(niketsu_msg);
        swarm_broadcast.handle_swarm_broadcast(peer_id, self)
    }

    fn handler(&mut self) -> &mut CommunicationHandler {
        &mut self.handler
    }
}
