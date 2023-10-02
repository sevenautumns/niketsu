use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use im::Vector;
use log::debug;
use ordered_float::OrderedFloat;
use url::Url;

use super::playlist::PlaylistVideo;
use super::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use super::{CoreModel, EventHandler};
use crate::player::MediaPlayerTraitExt;
use crate::playlist::Playlist;
use crate::rooms::RoomList;
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
    pub secure: bool,
}

impl Display for EndpointInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO rework the interiors ?
        if self.addr.contains("://") {
            return f.write_str(&self.addr);
        }
        let prefix = if self.secure { "wss://" } else { "ws://" };
        let addr = format!("{prefix}{}", self.addr);
        match Url::parse(&addr) {
            Ok(url) => f.write_str(url.as_str()),
            Err(_) => f.write_str(&self.addr),
        }
    }
}

impl EndpointInfo {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutgoingMessage {
    Join(NiketsuJoin),
    VideoStatus(NiketsuVideoStatus),
    Start(NiketsuStart),
    Pause(NiketsuPause),
    PlaybackSpeed(NiketsuPlaybackSpeed),
    Seek(NiketsuSeek),
    Select(NiketsuSelect),
    UserMessage(NiketsuUserMessage),
    Playlist(NiketsuPlaylist),
    UserStatus(NiketsuUserStatus),
}

#[enum_dispatch(EventHandler)]
#[derive(Clone, Debug)]
pub enum IncomingMessage {
    Connected(NiketsuConnected),
    UserStatusList(NiketsuUserStatusList),
    Start(NiketsuStart),
    Pause(NiketsuPause),
    PlaybackSpeed(NiketsuPlaybackSpeed),
    Seek(NiketsuSeek),
    Select(NiketsuSelect),
    UserMessage(NiketsuUserMessage),
    ServerMessage(NiketsuServerMessage),
    Playlist(NiketsuPlaylist),
}

#[derive(Debug, Clone, Copy)]
pub struct NiketsuConnected;

impl From<NiketsuConnected> for PlayerMessage {
    fn from(_: NiketsuConnected) -> Self {
        PlayerMessageInner {
            message: "Connected to Server".to_string(),
            source: MessageSource::Internal,
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for NiketsuConnected {
    fn handle(self, model: &mut CoreModel) {
        debug!("Server Connection Established");
        model.communicator.send(
            NiketsuJoin {
                password: model.password.clone().unwrap_or_default(),
                room: model.room.clone(),
                username: model.user.name.clone(),
            }
            .into(),
        );
        model.ui.player_message(PlayerMessage::from(self));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuJoin {
    pub password: String,
    pub room: String,
    pub username: String,
}

impl From<NiketsuJoin> for OutgoingMessage {
    fn from(value: NiketsuJoin) -> Self {
        Self::Join(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuVideoStatus {
    pub filename: Option<String>,
    pub position: Option<Duration>,
    pub speed: f64,
    pub paused: bool,
}

impl PartialEq for NiketsuVideoStatus {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        speed_self.eq(&speed_other)
            && self.filename.eq(&other.filename)
            && self.position.eq(&other.position)
            && self.paused.eq(&other.paused)
    }
}

impl Eq for NiketsuVideoStatus {}

impl From<NiketsuVideoStatus> for OutgoingMessage {
    fn from(value: NiketsuVideoStatus) -> Self {
        Self::VideoStatus(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuUserStatusList {
    pub rooms: BTreeMap<String, BTreeSet<NiketsuUserStatus>>,
}

impl EventHandler for NiketsuUserStatusList {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received User Status List");
        let rooms: BTreeMap<String, BTreeSet<UserStatus>> =
            BTreeMap::from_iter(self.rooms.into_iter().map(|(r, u)| {
                (
                    r,
                    BTreeSet::<UserStatus>::from_iter(u.into_iter().map(UserStatus::from)),
                )
            }));
        model.ui.room_list(RoomList::from(rooms));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuStart {
    pub actor: String,
}

impl From<NiketsuStart> for PlayerMessage {
    fn from(value: NiketsuStart) -> Self {
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

impl EventHandler for NiketsuStart {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Start");
        model.player.start();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<NiketsuStart> for OutgoingMessage {
    fn from(value: NiketsuStart) -> Self {
        Self::Start(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuPause {
    pub actor: String,
}

impl From<NiketsuPause> for PlayerMessage {
    fn from(value: NiketsuPause) -> Self {
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

impl EventHandler for NiketsuPause {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Pause");
        model.player.pause();
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<NiketsuPause> for OutgoingMessage {
    fn from(value: NiketsuPause) -> Self {
        Self::Pause(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuPlaybackSpeed {
    pub actor: String,
    pub speed: f64,
}

impl PartialEq for NiketsuPlaybackSpeed {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        speed_self.eq(&speed_other) && self.actor.eq(&other.actor)
    }
}

impl Eq for NiketsuPlaybackSpeed {}

impl From<NiketsuPlaybackSpeed> for PlayerMessage {
    fn from(value: NiketsuPlaybackSpeed) -> Self {
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

impl EventHandler for NiketsuPlaybackSpeed {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Speed Change");
        model.player.set_speed(self.speed);
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<NiketsuPlaybackSpeed> for OutgoingMessage {
    fn from(value: NiketsuPlaybackSpeed) -> Self {
        Self::PlaybackSpeed(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuSeek {
    pub actor: String,
    pub file: String,
    pub paused: bool,
    pub speed: f64,
    pub position: Duration,
}

impl PartialEq for NiketsuSeek {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        speed_self.eq(&speed_other)
            && self.actor.eq(&other.actor)
            && self.position.eq(&other.position)
            && self.paused.eq(&other.paused)
            && self.file.eq(&other.file)
    }
}

impl Eq for NiketsuSeek {}

impl From<NiketsuSeek> for PlayerMessage {
    fn from(value: NiketsuSeek) -> Self {
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

impl EventHandler for NiketsuSeek {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Seek: {self:?}");
        let playlist_video = PlaylistVideo::from(self.file.as_str());
        model.playlist.select_playing(&playlist_video);
        // TODO make this more readable
        if model
            .player
            .playing_video()
            .is_some_and(|v| v.name_str().eq(playlist_video.as_str()))
        {
            model.player.set_position(self.position);
            model.player.set_speed(self.speed);
            if self.paused {
                model.player.pause();
            } else {
                model.player.start();
            }
        } else {
            model
                .player
                .load_playlist_video(&playlist_video, model.database.all_files());
            model.ui.video_change(Some(playlist_video));
        }
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl From<NiketsuSeek> for OutgoingMessage {
    fn from(value: NiketsuSeek) -> Self {
        Self::Seek(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuSelect {
    pub actor: String,
    pub filename: Option<String>,
}

impl From<NiketsuSelect> for PlayerMessage {
    fn from(value: NiketsuSelect) -> Self {
        let actor = value.actor;
        let filename = value.filename;
        PlayerMessageInner {
            message: format!("{actor} selected {filename:?}"),
            source: MessageSource::UserAction(actor),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for NiketsuSelect {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Select: {self:?}");
        let playlist_video = self
            .filename
            .as_ref()
            .map(|f| PlaylistVideo::from(f.as_str()));
        if let Some(playlist_video) = &playlist_video {
            model.playlist.select_playing(playlist_video);
            let loaded = model
                .player
                .load_playlist_video(playlist_video, model.database.all_files());
            if !loaded {
                model.player.unload_video();
            };
        } else {
            model.playlist.unload_playing();
            model.player.unload_video();
        }
        model.ui.video_change(playlist_video);
        model.ui.player_message(PlayerMessage::from(self));
    }
}

impl From<NiketsuSelect> for OutgoingMessage {
    fn from(value: NiketsuSelect) -> Self {
        Self::Select(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuUserMessage {
    pub actor: String,
    pub message: String,
}

impl From<NiketsuUserMessage> for PlayerMessage {
    fn from(value: NiketsuUserMessage) -> Self {
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

impl EventHandler for NiketsuUserMessage {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received User Message: {self:?}");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<NiketsuUserMessage> for OutgoingMessage {
    fn from(value: NiketsuUserMessage) -> Self {
        Self::UserMessage(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuServerMessage {
    pub message: String,
}

impl From<NiketsuServerMessage> for PlayerMessage {
    fn from(value: NiketsuServerMessage) -> Self {
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

impl EventHandler for NiketsuServerMessage {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Server Message: {self:?}");
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiketsuPlaylist {
    pub actor: String,
    pub playlist: Vector<ArcStr>,
}

impl From<NiketsuPlaylist> for PlayerMessage {
    fn from(value: NiketsuPlaylist) -> Self {
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

impl EventHandler for NiketsuPlaylist {
    fn handle(self, model: &mut CoreModel) {
        debug!("Received Playlist");
        let playlist = Playlist::from_iter(self.playlist.iter());
        model.playlist.replace(playlist.clone());
        model.ui.playlist(playlist);
        model.ui.player_message(PlayerMessage::from(self))
    }
}

impl From<NiketsuPlaylist> for OutgoingMessage {
    fn from(value: NiketsuPlaylist) -> Self {
        Self::Playlist(value)
    }
}

#[derive(Debug, Clone, Eq)]
pub struct NiketsuUserStatus {
    pub ready: bool,
    pub username: String,
}

impl From<NiketsuUserStatus> for OutgoingMessage {
    fn from(value: NiketsuUserStatus) -> Self {
        Self::UserStatus(value)
    }
}

impl From<NiketsuUserStatus> for UserStatus {
    fn from(value: NiketsuUserStatus) -> Self {
        Self {
            name: value.username,
            ready: value.ready,
        }
    }
}

impl PartialEq for NiketsuUserStatus {
    fn eq(&self, other: &Self) -> bool {
        self.username.eq(&other.username)
    }
}

impl Ord for NiketsuUserStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.username.cmp(&other.username)
    }
}
impl PartialOrd for NiketsuUserStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.username.partial_cmp(&other.username)
    }
}
