use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::kad::{self, QueryId};
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, gossipsub};
use niketsu_core::communicator::{
    ChunkRequestMsg, ChunkResponseMsg, ConnectedMsg, FileRequestMsg, FileResponseMsg, PlaylistMsg,
    SelectMsg, StartMsg, UserMessageMsg, UserStatusListMsg, UserStatusMsg, VideoShareMsg,
    VideoStatusMsg,
};
use niketsu_core::playlist::Video;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::room::RoomName;
use niketsu_core::user::UserStatus;
use tracing::{debug, error, trace, warn};

use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, MessageResponse, Response, StatusResponse,
    SwarmHandler,
};
use crate::messages::NiketsuMessage;
use crate::p2p::MessageRequest;

#[enum_dispatch]
pub(crate) trait HostSwarmEventHandler {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler);
}

#[enum_dispatch(HostSwarmEventHandler)]
enum HostSwarmEvent {
    GossipSub(gossipsub::Event),
    MessageRequestResponse(request_response::Event<MessageRequest, MessageResponse>),
    Kademlia(kad::Event),
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
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                HostSwarmEvent::ConnectionEstablished(ConnectionEstablished { peer_id })
            }
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
                if let Err(error) = handler.handle_swarm_broadcast(message.data, propagation_source)
                {
                    error!(%error, "Failed to handle broadcast message");
                }
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
                    if let Err(error) = handler.handle_swarm_request(req, channel, peer) {
                        error!(%error, "Failed to handle incoming message");
                    }
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    if let Err(error) = handler.handle_swarm_response(response, peer) {
                        error!(%error, "Failed to handle incoming message");
                    }
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
        match self {
            kad::Event::OutboundQueryProgressed {
                id,
                result:
                    kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {
                        providers,
                        ..
                    })),
                ..
            } => match handler.current_request.get(&id) {
                Some(request) => {
                    if let Some(provider) = handler.pending_request_provider {
                        debug!("Already have provider");
                        handler
                            .swarm
                            .behaviour_mut()
                            .message_request_response
                            .send_request(
                                &provider,
                                MessageRequest(NiketsuMessage::FileRequest(request.clone())),
                            );
                    } else if let Some(provider) = providers.iter().next() {
                        debug!("Found providers");
                        handler.pending_request_provider = Some(*provider);

                        handler
                            .swarm
                            .behaviour_mut()
                            .message_request_response
                            .send_request(
                                provider,
                                MessageRequest(NiketsuMessage::FileRequest(request.clone())),
                            );
                    }
                }
                None => {
                    warn!("Found providers but no request?")
                }
            },
            kad::Event::OutboundQueryProgressed {
                result:
                    kad::QueryResult::GetProviders(Ok(
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. },
                    )),
                ..
            } => {
                debug!("No kademlia providers found");
                if let Err(err) =
                    handler
                        .message_sender
                        .send(NiketsuMessage::UserMessage(UserMessageMsg {
                            actor: arcstr::literal!("server"),
                            message: "No providers found for the requested file".into(),
                        }))
                {
                    debug!(?err, "Failed to send message to core");
                }
            }
            kad_event => debug!(?kad_event, "Received non handled kademlia event"),
        }
    }
}

struct ConnectionEstablished {
    peer_id: PeerId,
}

impl HostSwarmEventHandler for ConnectionEstablished {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        debug!(%self.peer_id, "New client established connection");
        if let Err(error) = handler.send_init_status(self.peer_id) {
            error!(%error, "Failed to send initial messages to client");
        }
    }
}

struct ConnectionClosed {
    peer_id: PeerId,
    cause: Option<ConnectionError>,
    endpoint: ConnectedPoint,
}

impl HostSwarmEventHandler for ConnectionClosed {
    fn handle_swarm_event(self, handler: &mut HostCommunicationHandler) {
        if handler.relay == (*self.endpoint.get_remote_address()) {
            error!(?self.endpoint, ?self.cause, "Connection of host to relay server closed");
            handler.core_receiver.close();
        } else if !handler.swarm.is_connected(&self.peer_id) {
            debug!("User connection stopped and user removed from map");
            let users = handler.users.clone();
            if let Some(status) = users.get(&self.peer_id) {
                handler.remove_peer(status, &self.peer_id);
                let status_list = NiketsuMessage::StatusList(handler.status_list.clone());
                if let Err(error) = handler.message_sender.send(status_list.clone()) {
                    error!(%error, "Failed to send status list to core");
                }
                if let Err(error) = handler
                    .swarm
                    .try_broadcast(handler.topic.clone(), status_list)
                {
                    error!(%error, "Failed to broadcast status list");
                }
            } else {
                warn!("Expected peer to be included in list");
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

#[enum_dispatch(HostCoreMessageHandler)]
enum HostCoreMessage {
    UserStatus(UserStatusMsg),
    VideoStatus(VideoStatusMsg),
    Playlist(PlaylistMsg),
    Select(SelectMsg),
    VideoShare(VideoShareMsg),
    ChunkRequest(ChunkRequestMsg),
    ChunkResponse(ChunkResponseMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    Other(NiketsuMessage),
}

impl HostCoreMessage {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::VideoStatus(msg) => HostCoreMessage::VideoStatus(msg),
            NiketsuMessage::Select(msg) => HostCoreMessage::Select(msg),
            NiketsuMessage::Playlist(msg) => HostCoreMessage::Playlist(msg),
            NiketsuMessage::Status(msg) => HostCoreMessage::UserStatus(msg),
            NiketsuMessage::FileRequest(msg) => HostCoreMessage::FileRequest(msg),
            NiketsuMessage::FileResponse(msg) => HostCoreMessage::FileResponse(msg),
            NiketsuMessage::ChunkRequest(msg) => HostCoreMessage::ChunkRequest(msg),
            NiketsuMessage::ChunkResponse(msg) => HostCoreMessage::ChunkResponse(msg),
            NiketsuMessage::VideoShare(msg) => HostCoreMessage::VideoShare(msg),
            msg => HostCoreMessage::Other(msg),
        }
    }
}

impl HostCoreMessageHandler for UserStatusMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        let peer_id = handler.host;
        handler.update_status(self, peer_id);
        handler.handle_all_users_ready(peer_id)?;
        let niketsu_msg = NiketsuMessage::StatusList(handler.status_list.clone());
        handler.message_sender.send(niketsu_msg.clone())?; // is this necessary?
        handler
            .swarm
            .try_broadcast(handler.topic.clone(), niketsu_msg)
    }
}

impl HostCoreMessageHandler for PlaylistMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.handle_new_playlist(&self, handler.host)?;
        handler.playlist = self.clone();
        handler
            .swarm
            .try_broadcast(handler.topic.clone(), self.into())
    }
}

impl HostCoreMessageHandler for VideoStatusMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.select.position = self.position.unwrap_or_default();
        handler
            .swarm
            .try_broadcast(handler.topic.clone(), self.into())
    }
}

impl HostCoreMessageHandler for SelectMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.select = self.clone();
        handler
            .swarm
            .try_broadcast(handler.topic.clone(), self.into())?;
        handler.handle_all_users_ready(handler.host)?;
        handler.reset_requests_responses();
        Ok(())
    }
}

impl HostCoreMessageHandler for VideoShareMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        match &self.video {
            Some(video) => {
                handler.current_response = Some(video.clone());
                handler.swarm.start_providing(video.clone())?;
            }
            None => {
                handler.reset_requests_responses();
            }
        }
        Ok(())
    }
}

impl HostCoreMessageHandler for ChunkRequestMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        //TODO handle issues with provider
        match handler.pending_request_provider {
            Some(provider) => handler.swarm.send_request(&provider, self.into()),
            None => bail!("No provider available for chunk request"),
        }
        Ok(())
    }
}

impl HostCoreMessageHandler for ChunkResponseMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        if let Some(channel) = handler.pending_chunk_responses.remove(&self.uuid) {
            handler
                .swarm
                .send_response(channel, MessageResponse(Response::Message(self.into())))?;
        }
        Ok(())
    }
}

impl HostCoreMessageHandler for FileRequestMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        let id = handler
            .swarm
            .behaviour_mut()
            .kademlia
            .get_providers(self.video.as_str().as_bytes().to_vec().into());
        handler.current_request.insert(id, self);
        Ok(())
    }
}

impl HostCoreMessageHandler for FileResponseMsg {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        if let Some(channel) = handler.pending_file_responses.remove(&self.uuid) {
            if self.video.is_none() {
                handler.swarm.send_response(
                    channel,
                    MessageResponse(Response::Status(StatusResponse::NotProvidingErr)),
                )
            } else {
                handler
                    .swarm
                    .send_response(channel, MessageResponse(Response::Message(self.into())))
            }
        } else {
            bail!("Cannot send file response if response channel does not exist");
        }
    }
}

impl HostCoreMessageHandler for NiketsuMessage {
    fn handle_core_message(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        handler.swarm.try_broadcast(handler.topic.clone(), self)
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

#[enum_dispatch(HostSwarmRequestHandler)]
enum HostSwarmRequest {
    UserStatus(UserStatusMsg),
    Playlist(PlaylistMsg),
    ChunkRequest(ChunkRequestMsg),
    FileRequest(FileRequestMsg),
    Other(NiketsuMessage),
}

impl HostSwarmRequest {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::Playlist(msg) => HostSwarmRequest::Playlist(msg),
            NiketsuMessage::Status(msg) => HostSwarmRequest::UserStatus(msg),
            NiketsuMessage::FileRequest(msg) => HostSwarmRequest::FileRequest(msg),
            NiketsuMessage::ChunkRequest(msg) => HostSwarmRequest::ChunkRequest(msg),
            msg => HostSwarmRequest::Other(msg),
        }
    }
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
            handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(err);
        }

        let msg = NiketsuMessage::StatusList(handler.status_list.clone());
        if let Err(err) = handler.message_sender.send(msg.clone()) {
            handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(anyhow::Error::from(err));
        }

        match handler.swarm.try_broadcast(handler.topic.clone(), msg) {
            Ok(_) => handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Ok)),
            ),
            Err(_) => handler.swarm.send_response(
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
        if let Err(err) = handler.message_sender.send(msg.clone()) {
            handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(anyhow::Error::from(err));
        }

        if let Err(err) = handler.swarm.try_broadcast(handler.topic.clone(), msg) {
            handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            return Err(err);
        }

        match handler.handle_new_playlist(&self, peer_id) {
            Ok(_) => {
                handler.playlist = self.clone();
                handler.swarm.send_response(
                    channel,
                    MessageResponse(Response::Status(StatusResponse::Ok)),
                )
            }
            Err(_) => handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
    }
}

impl HostSwarmRequestHandler for ChunkRequestMsg {
    fn handle_swarm_request(
        self,
        _peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        let msg = NiketsuMessage::ChunkRequest(self.clone());
        handler.message_sender.send(msg)?;
        handler.pending_chunk_responses.insert(self.uuid, channel);
        Ok(())
    }
}

impl HostSwarmRequestHandler for FileRequestMsg {
    fn handle_swarm_request(
        self,
        _peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        let msg = NiketsuMessage::FileRequest(self.clone());
        handler.message_sender.send(msg)?;
        handler.pending_file_responses.insert(self.uuid, channel);
        Ok(())
    }
}

impl HostSwarmRequestHandler for NiketsuMessage {
    fn handle_swarm_request(
        self,
        _peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut HostCommunicationHandler,
    ) -> Result<()> {
        handler.swarm.send_response(
            channel,
            MessageResponse(Response::Status(StatusResponse::Err)),
        )?;
        bail!("Host received unexpected direct message: {self:?}");
    }
}

#[enum_dispatch()]
trait HostSwarmResponseHandler {
    fn handle_swarm_response(self, handler: &mut HostCommunicationHandler) -> Result<()>;
}

#[enum_dispatch(HostSwarmResponseHandler)]
enum HostSwarmResponse {
    Status(StatusResponse),
    ChunkResponse(ChunkResponseMsg),
    FileResponse(FileResponseMsg),
    Other(NiketsuMessage),
}

impl HostSwarmResponse {
    fn from(message: MessageResponse) -> Self {
        match message.0 {
            Response::Status(msg) => HostSwarmResponse::Status(msg),
            Response::Message(niketsu_message) => match niketsu_message {
                NiketsuMessage::FileResponse(msg) => HostSwarmResponse::FileResponse(msg),
                NiketsuMessage::ChunkResponse(msg) => HostSwarmResponse::ChunkResponse(msg),
                msg => HostSwarmResponse::Other(msg),
            },
        }
    }
}

impl HostSwarmResponseHandler for StatusResponse {
    fn handle_swarm_response(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        if self == StatusResponse::NotProvidingErr {
            handler.pending_request_provider.take();
        }
        Ok(())
    }
}

impl HostSwarmResponseHandler for ChunkResponseMsg {
    fn handle_swarm_response(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        let msg = NiketsuMessage::ChunkResponse(self);
        handler
            .message_sender
            .send(msg)
            .map_err(anyhow::Error::from)
    }
}

impl HostSwarmResponseHandler for FileResponseMsg {
    fn handle_swarm_response(self, handler: &mut HostCommunicationHandler) -> Result<()> {
        let msg = NiketsuMessage::FileResponse(self);
        handler
            .message_sender
            .send(msg)
            .map_err(anyhow::Error::from)
    }
}

impl HostSwarmResponseHandler for NiketsuMessage {
    fn handle_swarm_response(self, _handler: &mut HostCommunicationHandler) -> Result<()> {
        bail!("Did not expect response message {self:?}")
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
        handler.select = self.clone();
        handler.message_sender.send(msg)?;
        handler.handle_all_users_ready(peer_id)?;
        handler.reset_requests_responses();
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
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    relay: Multiaddr,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    status_list: UserStatusListMsg,
    playlist: PlaylistMsg,
    select: SelectMsg,
    users: HashMap<PeerId, UserStatus>,
    current_request: HashMap<QueryId, FileRequestMsg>,
    pending_request_provider: Option<PeerId>,
    pending_chunk_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    pending_file_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    current_response: Option<Video>,
}

impl HostCommunicationHandler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay: Multiaddr,
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
        Self {
            swarm,
            topic,
            host,
            relay,
            core_receiver,
            message_sender,
            status_list: UserStatusListMsg {
                room_name: room,
                users: BTreeSet::default(),
            },
            playlist,
            users: HashMap::default(),
            select,
            current_request: Default::default(),
            pending_request_provider: None,
            pending_chunk_responses: Default::default(),
            pending_file_responses: Default::default(),
            current_response: None,
        }
    }

    fn send_init_status(&mut self, peer_id: PeerId) -> Result<()> {
        self.swarm
            .send_request(&peer_id, NiketsuMessage::Playlist(self.playlist.clone()));

        self.swarm
            .send_request(&peer_id, NiketsuMessage::Select(self.select.clone()));

        self.swarm.try_broadcast(
            self.topic.clone(),
            NiketsuMessage::StatusList(self.status_list.clone()),
        )
    }

    fn update_status(&mut self, status: UserStatus, peer_id: PeerId) {
        self.status_list.users.replace(status.clone());
        self.users.insert(peer_id, status);
    }

    fn remove_peer(&mut self, status: &UserStatus, peer_id: &PeerId) {
        self.status_list.users.remove(status);
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

    fn is_established_user(&self, peer_id: PeerId) -> bool {
        self.users.contains_key(&peer_id)
    }

    fn username_exists(&self, name: ArcStr) -> bool {
        self.status_list.users.iter().any(|u| u.name == name)
    }

    fn handle_status(&mut self, status: UserStatus, peer_id: PeerId) {
        let mut new_status = status.clone();

        if self.is_established_user(peer_id) {
            if !self.username_exists(status.name.clone()) {
                // user name change, remove old status and update user map
                let users = self.users.clone(); // is there any other way to avoid mut/immut borrow?
                let old_status = users.get(&peer_id).expect("User should exist");
                self.remove_peer(old_status, &peer_id);
            }
            // otherwise, typical user status update, only need to update map & list
        } else {
            // new user
            if self.username_exists(status.name.clone()) {
                // username needs to be force changed to avoid duplicate user names
                let new_username = self.roll_new_username(status.name.clone());
                new_status = UserStatus {
                    name: new_username,
                    ready: status.ready,
                };
                self.swarm
                    .send_request(&peer_id, NiketsuMessage::Status(new_status.clone()));
            }
        }

        self.update_status(new_status, peer_id);
    }

    fn handle_all_users_ready(&mut self, peer_id: PeerId) -> Result<()> {
        if self.all_users_ready() {
            debug!("All users area ready. Publishing start to gossipsub");
            let mut start_msg = NiketsuMessage::Start(StartMsg {
                actor: arcstr::literal!("Sever"),
            });
            if let Some(user) = self.users.get(&peer_id) {
                start_msg = NiketsuMessage::Start(StartMsg {
                    actor: user.name.clone(),
                });
            }
            self.message_sender.send(start_msg.clone())?;
            self.swarm.try_broadcast(self.topic.clone(), start_msg)?;
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
            self.message_sender.send(msg.clone())?;
            self.swarm.try_broadcast(self.topic.clone(), msg)?;
            self.handle_all_users_ready(peer_id)?;
        }
        Ok(())
    }

    fn reset_requests_responses(&mut self) {
        if let Some(video) = self.current_response.clone() {
            self.swarm.stop_providing(video);
        }
        self.pending_chunk_responses = Default::default();
        self.current_response = None;
        self.current_request = Default::default();
        self.pending_request_provider = None;
    }
}

#[async_trait]
impl CommunicationHandler for HostCommunicationHandler {
    async fn run(&mut self) {
        if let Err(error) = self.message_sender.send(ConnectedMsg.into()) {
            warn!(%error, "Failed to send connected message to core");
        }

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_swarm_event(event),
                msg = self.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "core message");
                        if let Err(error) = self.handle_core_message(msg) {
                            error!(%error, "Handling message caused error");
                        }
                    },
                    None => {
                        debug!("Channel of core closed. Stopping p2p client event loop");
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
        debug!(host = %self.host, ?msg, "Handling core message");
        let core_message = HostCoreMessage::from(msg);
        core_message.handle_core_message(self)
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Handling request message from swarm");
        let swarm_request = HostSwarmRequest::from(msg);
        swarm_request.handle_swarm_request(peer_id, channel, self)
    }

    fn handle_swarm_response(&mut self, msg: MessageResponse, peer_id: PeerId) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Handling response message from swarm");
        let swarm_response = HostSwarmResponse::from(msg);
        swarm_response.handle_swarm_response(self)
    }

    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        debug!(message = ?niketsu_msg, "Handling broadcast message from swarm");
        let swarm_broadcast = HostSwarmBroadcast::from(niketsu_msg);
        swarm_broadcast.handle_swarm_broadcast(peer_id, self)
    }
}
