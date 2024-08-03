use std::collections::BTreeSet;
use std::fmt::Display;
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use tracing::trace;

use super::playlist::Video;
use super::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use super::{CoreModel, EventHandler};
use crate::player::MediaPlayerTrait;
use crate::playlist::file::PlaylistBrowser;
use crate::playlist::Playlist;
use crate::room::{RoomName, UserList};
use crate::user::UserStatus;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommunicatorTrait: std::fmt::Debug + Send {
    fn connect(&mut self, connect: EndpointInfo);
    fn send(&mut self, msg: OutgoingMessage);
    async fn receive(&mut self) -> IncomingMessage;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub addr: String,
    pub room: RoomName,
    pub password: String,
}

impl Display for EndpointInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO rework the interiors ?
        f.write_str(&self.addr)
    }
}

impl EndpointInfo {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutgoingMessage {
    VideoStatus(VideoStatusMsg),
    Start(StartMsg),
    Pause(PauseMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    Playlist(PlaylistMsg),
    UserStatus(UserStatusMsg),
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
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConnectedMsg;

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
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.ui.player_message(PlayerMessage::from(self));
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
        let speed_other = OrderedFloat(self.speed);
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
        let (Some(pos), Some(_)) = (self.position, self.video) else {
            trace!("video status sent without position or video");
            return;
        };
        model.player.reconcile(pos)
    }
}

impl From<VideoStatusMsg> for OutgoingMessage {
    fn from(value: VideoStatusMsg) -> Self {
        Self::VideoStatus(value)
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

impl From<StartMsg> for OutgoingMessage {
    fn from(value: StartMsg) -> Self {
        Self::Start(value)
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

impl From<PauseMsg> for OutgoingMessage {
    fn from(value: PauseMsg) -> Self {
        Self::Pause(value)
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
        let speed_other = OrderedFloat(self.speed);
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

impl From<PlaybackSpeedMsg> for OutgoingMessage {
    fn from(value: PlaybackSpeedMsg) -> Self {
        Self::PlaybackSpeed(value)
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
        let playlist_video = Video::from(self.video.as_str());
        model.playlist.select_playing(&playlist_video);
        PlaylistBrowser::save(&model.config.room, &model.playlist);
        // TODO make this more readable
        if model
            .player
            .playing_video()
            .is_some_and(|v| v.as_str().eq(playlist_video.as_str()))
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

impl From<SeekMsg> for OutgoingMessage {
    fn from(value: SeekMsg) -> Self {
        Self::Seek(value)
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
        let playlist_video = self.video.as_ref().map(|f| Video::from(f.as_str()));
        if let Some(playlist_video) = playlist_video.clone() {
            model.playlist.select_playing(&playlist_video);
            PlaylistBrowser::save(&model.config.room, &model.playlist);
            model
                .player
                .load_video(playlist_video, self.position, model.database.all_files());
        } else {
            model.playlist.unload_playing();
            PlaylistBrowser::save(&model.config.room, &model.playlist);
            model.player.unload_video();
        }
        model.ui.video_change(playlist_video);
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl From<SelectMsg> for OutgoingMessage {
    fn from(value: SelectMsg) -> Self {
        Self::Select(value)
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

impl From<UserMessageMsg> for OutgoingMessage {
    fn from(value: UserMessageMsg) -> Self {
        Self::UserMessage(value)
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
        model.playlist.replace(self.playlist.clone());
        PlaylistBrowser::save(&model.config.room, &model.playlist);
        model.ui.playlist(self.playlist.clone());
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<PlaylistMsg> for OutgoingMessage {
    fn from(value: PlaylistMsg) -> Self {
        Self::Playlist(value)
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

impl From<UserStatus> for OutgoingMessage {
    fn from(value: UserStatus) -> Self {
        Self::UserStatus(value)
    }
}
