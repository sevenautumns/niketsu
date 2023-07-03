use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use async_trait::async_trait;

#[async_trait]
pub trait CommunicatorTrait {
    fn new(addr: String) -> Self;
    fn send(&mut self, msg: OutgoingMessage);
    async fn receive(&mut self) -> IncomingMessage;
}

#[derive(Clone, Debug)]
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

#[derive(Debug, Clone)]
pub struct NiketsuConnected;

impl From<NiketsuConnected> for IncomingMessage {
    fn from(value: NiketsuConnected) -> Self {
        Self::Connected(value)
    }
}

#[derive(Debug, Clone)]
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

impl From<NiketsuVideoStatus> for OutgoingMessage {
    fn from(value: NiketsuVideoStatus) -> Self {
        Self::VideoStatus(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuUserStatusList {
    pub rooms: BTreeMap<String, BTreeSet<NiketsuUserStatus>>,
}

impl From<NiketsuUserStatusList> for IncomingMessage {
    fn from(value: NiketsuUserStatusList) -> Self {
        Self::UserStatusList(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuStart {
    pub actor: String,
}

impl From<NiketsuStart> for IncomingMessage {
    fn from(value: NiketsuStart) -> Self {
        Self::Start(value)
    }
}

impl From<NiketsuStart> for OutgoingMessage {
    fn from(value: NiketsuStart) -> Self {
        Self::Start(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuPause {
    pub actor: String,
}

impl From<NiketsuPause> for IncomingMessage {
    fn from(value: NiketsuPause) -> Self {
        Self::Pause(value)
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

impl From<NiketsuPlaybackSpeed> for IncomingMessage {
    fn from(value: NiketsuPlaybackSpeed) -> Self {
        Self::PlaybackSpeed(value)
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
    pub position: Duration,
}

impl From<NiketsuSeek> for IncomingMessage {
    fn from(value: NiketsuSeek) -> Self {
        Self::Seek(value)
    }
}

impl From<NiketsuSeek> for OutgoingMessage {
    fn from(value: NiketsuSeek) -> Self {
        Self::Seek(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuSelect {
    pub actor: String,
    pub filename: Option<String>,
}

impl From<NiketsuSelect> for IncomingMessage {
    fn from(value: NiketsuSelect) -> Self {
        Self::Select(value)
    }
}

impl From<NiketsuSelect> for OutgoingMessage {
    fn from(value: NiketsuSelect) -> Self {
        Self::Select(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuUserMessage {
    pub actor: String,
    pub message: String,
}

impl From<NiketsuUserMessage> for IncomingMessage {
    fn from(value: NiketsuUserMessage) -> Self {
        Self::UserMessage(value)
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

impl From<NiketsuServerMessage> for IncomingMessage {
    fn from(value: NiketsuServerMessage) -> Self {
        Self::ServerMessage(value)
    }
}

#[derive(Debug, Clone)]
pub struct NiketsuPlaylist {
    pub actor: String,
    pub playlist: Vec<String>,
}

impl From<NiketsuPlaylist> for IncomingMessage {
    fn from(value: NiketsuPlaylist) -> Self {
        Self::Playlist(value)
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
