use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use multiaddr::Multiaddr;
pub use multiaddr::PeerId;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use super::playlist::Video;
use super::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use super::{CoreModel, EventHandler};
use crate::player::MediaPlayerTrait;
use crate::playlist::Playlist;
use crate::room::{RoomName, UserList};
use crate::sync::ClientSync;
use crate::user::UserStatus;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommunicatorTrait: std::fmt::Debug + Send {
    fn connect(&mut self, connect: EndpointInfo);
    fn send(&mut self, msg: OutgoingMessage);
    async fn receive(&mut self) -> IncomingMessage;
    fn has_endpoint(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub addr: Multiaddr,
    pub room: RoomName,
    pub password: String,
}

/// What the core asks the communicator to deliver. The communicator decides
/// only *how* a message travels for its role (gossip broadcast, or a direct
/// request to the host); *whether* and *what* to send is decided here.
///
/// Messages are grouped by the role that may send them, so each role's
/// handler matches exhaustively over its own group. Which role the core
/// holds is a runtime fact (the relay assigns it on connect, and a message
/// can still be queued while the role changes), so a message of the other
/// role's group can still reach a handler; it is dropped there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutgoingMessage {
    Room(RoomMessage),
    Host(HostInstruction),
    Client(ClientRequest),
}

/// Sent the same way whichever role this core holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomMessage {
    Start(StartMsg),
    Pause(PauseMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    ChunkRequest(ChunkRequestMsg),
    ChunkResponse(ChunkResponseMsg),
    VideoShareChange(VideoShareMsg),
}

/// What only the host sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostInstruction {
    /// The host's clock, the room's reference.
    VideoStatus(VideoStatusMsg),
    /// The room's playlist, sent to the room.
    Playlist(PlaylistMsg),
    /// The host's view of who is in the room, sent to the room.
    UserStatusList(UserStatusListMsg),
    /// A message for exactly one peer rather than the room.
    ToPeer(ToPeerMsg),
    HostHandover(HostHandoverRequest),
}

/// What only a client sends, each a request to the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientRequest {
    /// An edit of the room's playlist, for the host to adopt.
    Playlist(PlaylistMsg),
    /// The client's own status.
    UserStatus(UserStatusMsg),
}

impl From<RoomMessage> for OutgoingMessage {
    fn from(value: RoomMessage) -> Self {
        Self::Room(value)
    }
}

impl From<HostInstruction> for OutgoingMessage {
    fn from(value: HostInstruction) -> Self {
        Self::Host(value)
    }
}

impl From<ClientRequest> for OutgoingMessage {
    fn from(value: ClientRequest) -> Self {
        Self::Client(value)
    }
}

/// `From<$msg> for OutgoingMessage` for messages whose sending role follows
/// from their type. A playlist is sent by both roles with different meaning,
/// so it is wrapped explicitly.
macro_rules! outgoing_from {
    ($($msg:ty => $outer:ident($group:ident::$variant:ident)),* $(,)?) => {
        $(impl From<$msg> for OutgoingMessage {
            fn from(value: $msg) -> Self {
                Self::$outer($group::$variant(value))
            }
        })*
    };
}

outgoing_from! {
    StartMsg => Room(RoomMessage::Start),
    PauseMsg => Room(RoomMessage::Pause),
    PlaybackSpeedMsg => Room(RoomMessage::PlaybackSpeed),
    SeekMsg => Room(RoomMessage::Seek),
    SelectMsg => Room(RoomMessage::Select),
    UserMessageMsg => Room(RoomMessage::UserMessage),
    FileRequestMsg => Room(RoomMessage::FileRequest),
    FileResponseMsg => Room(RoomMessage::FileResponse),
    ChunkRequestMsg => Room(RoomMessage::ChunkRequest),
    ChunkResponseMsg => Room(RoomMessage::ChunkResponse),
    VideoShareMsg => Room(RoomMessage::VideoShareChange),
    VideoStatusMsg => Host(HostInstruction::VideoStatus),
    UserStatusListMsg => Host(HostInstruction::UserStatusList),
    ToPeerMsg => Host(HostInstruction::ToPeer),
    HostHandoverRequest => Host(HostInstruction::HostHandover),
    UserStatusMsg => Client(ClientRequest::UserStatus),
}

#[enum_dispatch(EventHandler)]
#[derive(Clone, Debug)]
pub enum IncomingMessage {
    VideoStatus(VideoStatusMsg),
    Connected(ConnectedMsg),
    ConnectionError(ConnectionErrorMsg),
    UserStatusList(UserStatusListMsg),
    Start(StartMsg),
    Pause(PauseMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    ServerMessage(ServerMessageMsg),
    Playlist(PlaylistMsg),
    UserStatus(UserStatusMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    ChunkRequest(ChunkRequestMsg),
    ChunkResponse(ChunkResponseMsg),
    VideoProviderStopped(VideoProviderStoppedMsg),
    HostHandover(HostHandoverMsg),
    PeerConnected(PeerConnectedMsg),
    PeerDisconnected(PeerDisconnectedMsg),
    PeerStatus(PeerStatusMsg),
    HostRoundTrip(HostRoundTripMsg),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedMsg {
    pub is_host: bool,
}

impl From<ConnectedMsg> for PlayerMessage {
    fn from(_: ConnectedMsg) -> Self {
        PlayerMessageInner {
            message: "connected to server".to_string(),
            source: MessageSource::Internal,
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for ConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("server connection established");
        if self.is_host {
            model.become_host();
        } else {
            model.host = None;
        }
        model.joined_room = Some(model.config.room.clone());
        // round trips of an earlier connection say nothing about this one
        model.sync = ClientSync::default();
        model.publish_status();
        model.ui.is_host(self.is_host);
        model.ui.player_message(PlayerMessage::from(self));
    }
}

/// A peer reachable over a direct connection, so room messages can flow.
/// The host greets it with the room state; this repeats for every new
/// direct connection to the same peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerConnectedMsg {
    pub peer: PeerId,
}

impl EventHandler for PeerConnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, "peer connected");
        model.greet_peer(self.peer);
    }
}

/// The last connection to a peer closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerDisconnectedMsg {
    pub peer: PeerId,
}

impl EventHandler for PeerDisconnectedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, "peer disconnected");
        model.peer_left(self.peer);
    }
}

/// A peer's own status, as sent to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStatusMsg {
    pub peer: PeerId,
    pub status: UserStatus,
}

impl EventHandler for PeerStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(peer = %self.peer, status = ?self.status, "peer status");
        model.peer_status(self.peer, self.status);
    }
}

/// One measured round trip to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRoundTripMsg {
    pub rtt: Duration,
}

impl EventHandler for HostRoundTripMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(rtt = ?self.rtt, "host round trip");
        model.sync.delay.push(self.rtt);
    }
}

/// What the host can address to a single peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectMessage {
    Playlist(PlaylistMsg),
    Select(SelectMsg),
    /// A rename the host forces on the peer.
    UserStatus(UserStatusMsg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToPeerMsg {
    pub peer: PeerId,
    pub message: DirectMessage,
}

impl ToPeerMsg {
    pub fn new(peer: PeerId, message: DirectMessage) -> Self {
        Self { peer, message }
    }
}

impl From<DirectMessage> for IncomingMessage {
    fn from(value: DirectMessage) -> Self {
        match value {
            DirectMessage::Playlist(msg) => msg.into(),
            DirectMessage::Select(msg) => msg.into(),
            DirectMessage::UserStatus(msg) => msg.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionErrorMsg(pub String);

impl From<ConnectionErrorMsg> for PlayerMessage {
    fn from(error: ConnectionErrorMsg) -> Self {
        PlayerMessageInner {
            message: format!("Connection Error: {}", error.0),
            source: MessageSource::Internal,
            level: MessageLevel::Error,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for ConnectionErrorMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("server connection established");
        //TODO?
        model.ui.player_message(PlayerMessage::from(self));
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStatusMsg {
    pub video: Option<Video>,
    #[serde(with = "serde_millis")]
    pub position: Option<Duration>,
    pub speed: f64,
    pub paused: bool,
    pub file_loaded: bool,
    pub cache: bool,
}

impl PartialEq for VideoStatusMsg {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(other.speed);
        speed_self.eq(&speed_other)
            && self.video.eq(&other.video)
            && self.position.eq(&other.position)
            && self.paused.eq(&other.paused)
    }
}

impl Eq for VideoStatusMsg {}

impl EventHandler for VideoStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received video status for reconciliation");

        if model.sync.is_seeking() {
            debug!("can not determine client position during seek");
            return;
        }

        // PlaybackSpeed only announces a change, so the heartbeat is the only
        // place the room's speed is repeated: a peer that joined a session
        // already running at another speed learns it here, and would otherwise
        // keep playing at its own speed and drift out of sync every few
        // seconds. The speed is independent of what is loaded, so it is
        // adopted even while the host has no video.
        model.player.set_speed(self.speed);

        let (Some(pos), Some(_)) = (self.position, self.video) else {
            trace!("video status sent without position or video: unloading video");
            model.player.unload_video();
            return;
        };

        // The host is only a valid play/position reference once it has the file
        // loaded. While it is still loading it reports a stale paused=false and
        // a frozen position from the previous video — following either would
        // start our (possibly faster-loaded) copy and then yank it back every
        // heartbeat. Hold paused and wait until the host is actually playing.
        if !self.file_loaded {
            model.player.pause();
            return;
        }

        //TODO check if current video is not the same as host?
        if let Some(paused) = model.player.is_paused() {
            match (paused, self.paused) {
                (true, false) => model.player.start(),
                (false, true) => model.player.pause(),
                _ => {}
            }
        }

        let pos = model.sync.host_position(pos, self.paused);
        model.player.reconcile(pos)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusListMsg {
    pub room_name: RoomName,
    pub users: BTreeSet<UserStatus>,
}

impl EventHandler for UserStatusListMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received user status list");
        model.ui.user_list(UserList::from(self));
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartMsg {
    pub actor: ArcStr,
}

impl From<StartMsg> for PlayerMessage {
    fn from(value: StartMsg) -> Self {
        let actor = value.actor;
        PlayerMessageInner {
            message: format!("{actor} started playback"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for StartMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received start");
        model.player.start();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PauseMsg {
    pub actor: ArcStr,
}

impl From<PauseMsg> for PlayerMessage {
    fn from(value: PauseMsg) -> Self {
        let actor = value.actor;
        PlayerMessageInner {
            message: format!("{actor} paused playback"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for PauseMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received pause");
        model.player.pause();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSpeedMsg {
    pub actor: ArcStr,
    pub speed: f64,
}

impl PartialEq for PlaybackSpeedMsg {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(other.speed);
        speed_self.eq(&speed_other) && self.actor.eq(&other.actor)
    }
}

impl Eq for PlaybackSpeedMsg {}

impl From<PlaybackSpeedMsg> for PlayerMessage {
    fn from(value: PlaybackSpeedMsg) -> Self {
        let actor = value.actor;
        let speed = value.speed;
        PlayerMessageInner {
            message: format!("{actor} changed playback speed to {speed}"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for PlaybackSpeedMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received speed change");
        model.player.set_speed(self.speed);
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SeekMsg {
    pub actor: ArcStr,
    pub video: Video,
    #[serde(with = "serde_millis")]
    pub position: Duration,
}

impl From<SeekMsg> for PlayerMessage {
    fn from(value: SeekMsg) -> Self {
        let actor = value.actor;
        let position = value.position;
        PlayerMessageInner {
            message: format!("{actor} seeked to {position:?}"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for SeekMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(seek = ?self, "received");
        model.sync.seek_started();
        let playlist_video = Video::from(self.video.as_str());
        model.playlist.select_playing(&playlist_video);
        model.save_playlist();
        if model
            .player
            .playing_video()
            .is_some_and(|v| playlist_video.eq(&v))
        {
            model.player.set_position(self.position);
        } else {
            model.player.load_video(
                playlist_video.clone(),
                self.position,
                model.database.all_files(),
            );
            model.ui.video_change(Some(playlist_video));
        }
        model.ui.player_message(PlayerMessage::from(self));
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectMsg {
    pub actor: ArcStr,
    #[serde(with = "serde_millis")]
    pub position: Duration,
    pub video: Option<Video>,
}

impl From<SelectMsg> for PlayerMessage {
    fn from(value: SelectMsg) -> Self {
        let actor = value.actor;
        let message = if let Some(video) = value.video {
            format!("{actor} selected {}", video.as_str())
        } else {
            format!("{actor} unselected video")
        };
        PlayerMessageInner {
            message,
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for SelectMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(select = ?self, "received");
        model.receive_select(self);
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageMsg {
    pub actor: ArcStr,
    pub message: String,
}

impl From<UserMessageMsg> for PlayerMessage {
    fn from(value: UserMessageMsg) -> Self {
        let actor = value.actor;
        let message = value.message;
        PlayerMessageInner {
            message,
            source: MessageSource::UserMessage(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for UserMessageMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(user_message = ?self, "received");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerMessageMsg {
    pub message: String,
}

impl From<ServerMessageMsg> for PlayerMessage {
    fn from(value: ServerMessageMsg) -> Self {
        let message = value.message;
        PlayerMessageInner {
            message,
            source: MessageSource::Server,
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for ServerMessageMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(server_message = ?self, "received");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMsg {
    pub actor: ArcStr,
    #[serde(flatten)]
    pub playlist: Playlist,
}

impl From<PlaylistMsg> for PlayerMessage {
    fn from(value: PlaylistMsg) -> Self {
        let actor = value.actor;
        PlayerMessageInner {
            message: format!("{actor} changed playlist"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for PlaylistMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received playlist");
        model.receive_playlist(self);
    }
}

pub type UserStatusMsg = UserStatus;

impl EventHandler for UserStatusMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("username changed by server");
        model.config.username.clone_from(&self.name);
        model.ui.username_change(self.name.clone());
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl From<UserStatus> for PlayerMessage {
    fn from(value: UserStatus) -> Self {
        let name = value.name;
        PlayerMessageInner {
            message: format!("Username changed to {name}"),
            source: MessageSource::Server,
            level: MessageLevel::Warn,
            timestamp: Local::now(),
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRequestMsg {
    pub uuid: uuid::Uuid,
    pub actor: Option<ArcStr>,
    pub video: Video,
    pub range: RangeInclusive<u64>,
}

impl EventHandler for ChunkRequestMsg {
    fn handle(self, model: &mut CoreModel) {
        let start = *self.range.start();
        let len = self.range.end() - self.range.start() + 1;
        model
            .video_provider
            .request_chunk(self.uuid, self.video.as_str(), start, len);
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkResponseMsg {
    pub uuid: uuid::Uuid,
    pub actor: Option<ArcStr>,
    pub video: Video,
    pub start: u64,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for ChunkResponseMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkResponseMsg")
            .field("uuid", &self.uuid)
            .field("actor", &self.actor)
            .field("video", &self.video)
            .field("start", &self.start)
            .field("bytes", &"[not shown]")
            .finish()
    }
}

impl EventHandler for ChunkResponseMsg {
    fn handle(self, model: &mut CoreModel) {
        model
            .video_server
            .insert_chunk(self.video.as_str(), self.start, self.bytes)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileRequestMsg {
    // TODO check if we need the uuid
    pub uuid: uuid::Uuid,
    pub actor: ArcStr,
    pub video: Video,
}

impl From<FileRequestMsg> for PlayerMessage {
    fn from(value: FileRequestMsg) -> Self {
        let actor = value.actor;
        let video = value.video;
        PlayerMessageInner {
            message: format!("Received video request for {video:?} for {actor:?}"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for FileRequestMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!(file_request = ?self, "received");
        model.ui.player_message(self.clone().into());
        let failed_response = OutgoingMessage::from(FileResponseMsg {
            uuid: self.uuid,
            actor: model.config.username.clone(),
            video: None,
            size: 0,
        });

        let Some(file_name) = model.video_provider.file_name() else {
            model.communicator.send(failed_response);
            let msg = OutgoingMessage::from(UserMessageMsg {
                actor: arcstr::literal!("server message"),
                message: "File provider is currently not sharing. Try again later".into(),
            });
            model.communicator.send(msg);
            return;
        };

        if !file_name.as_str().eq(self.video.as_str()) {
            model.communicator.send(failed_response);
            let msg = OutgoingMessage::from(UserMessageMsg {
                actor: arcstr::literal!("server message"),
                message: "File provider is currently providing another file. Try again later"
                    .into(),
            });
            model.communicator.send(msg);
            return;
        }

        let Some(size) = model.video_provider.size() else {
            model.communicator.send(failed_response);
            let msg = OutgoingMessage::from(UserMessageMsg {
                actor: arcstr::literal!("server message"),
                message: "File provider has no info on the file. Try again later".into(),
            });
            model.communicator.send(msg);
            return;
        };

        let success_response = FileResponseMsg {
            uuid: self.uuid,
            actor: model.config.username.clone(),
            video: Some(file_name.as_str().into()),
            size,
        };
        model.communicator.send(success_response.into());
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileResponseMsg {
    pub uuid: uuid::Uuid,
    pub actor: ArcStr,
    pub video: Option<Video>,
    pub size: u64,
}

impl From<FileResponseMsg> for PlayerMessage {
    fn from(value: FileResponseMsg) -> Self {
        let actor = value.actor;
        let video = value.video;
        PlayerMessageInner {
            message: format!("Received video response for {video:?} from {actor:?}"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Debug,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for FileResponseMsg {
    fn handle(self, model: &mut CoreModel) {
        trace!("received file response message");
        model.ui.player_message(self.clone().into());
        match self.video {
            Some(video) => {
                let video = ArcStr::from(video.as_str());
                model.video_server.start_server(video, self.size);
            }
            None => {
                debug!("file response contains no video");
                // a server still up from an earlier stream would keep
                // serving a file nobody will ever send chunks for
                model.video_server.stop_server();
            }
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoShareMsg {
    pub video: Option<Video>,
}

impl VideoShareMsg {
    pub fn new(video: Video) -> Self {
        Self { video: Some(video) }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostHandoverMsg {
    pub new_host: ArcStr,
}

impl EventHandler for HostHandoverMsg {
    fn handle(self, model: &mut CoreModel) {
        model.ui.player_message(
            PlayerMessageInner {
                message: format!("{} is now the host", self.new_host),
                source: MessageSource::Internal,
                level: MessageLevel::Success,
                timestamp: Local::now(),
            }
            .into(),
        );
        if self.new_host == model.config.username {
            model.communicator.connect(EndpointInfo {
                addr: model.config.addr(),
                room: model.config.room.clone(),
                password: model.config.password.clone(),
            });
        }
    }
}

/// The host hands the room over to `peer`, which goes by `new_host`. The
/// communicator asks the relay to transfer the room and announces the new
/// host to the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHandoverRequest {
    pub peer: PeerId,
    pub new_host: ArcStr,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoProviderStoppedMsg;

impl EventHandler for VideoProviderStoppedMsg {
    fn handle(self, model: &mut CoreModel) {
        model.video_server.stop_server();
    }
}
