use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::communicator::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    Ping(PingMessage),
    Join(JoinMessage),
    VideoStatus(VideoStatusMessage),
    StatusList(StatusListMessage),
    Pause(PauseMessage),
    Start(StartMessage),
    PlaybackSpeed(PlaybackSpeedMessage),
    Seek(SeekMessage),
    Select(SelectMessage),
    UserMessage(UserMessageMessage),
    ServerMessage(ServerMessageMessage),
    Playlist(PlaylistMessage),
    Status(UserStatusMessage),
}

impl TryFrom<NiketsuMessage> for IncomingMessage {
    type Error = NiketsuMessage;

    fn try_from(value: NiketsuMessage) -> Result<Self, Self::Error> {
        match value {
            NiketsuMessage::StatusList(m) => Ok(m.into()),
            NiketsuMessage::Pause(m) => Ok(m.into()),
            NiketsuMessage::Start(m) => Ok(m.into()),
            NiketsuMessage::PlaybackSpeed(m) => Ok(m.into()),
            // NiketsuMessage::Seek(m) => Ok(m.into()),
            NiketsuMessage::Select(m) => Ok(m.into()),
            NiketsuMessage::UserMessage(m) => Ok(m.into()),
            NiketsuMessage::ServerMessage(m) => Ok(m.into()),
            NiketsuMessage::Playlist(m) => Ok(m.into()),
            value => Err(value),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PingMessage {
    uuid: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JoinMessage {
    pub(super) password: String,
    pub(super) room: String,
    pub(super) username: String,
}

impl From<NiketsuJoin> for NiketsuMessage {
    fn from(value: NiketsuJoin) -> Self {
        Self::Join(JoinMessage {
            password: value.password,
            room: value.room,
            username: value.username,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VideoStatusMessage {
    pub(super) filename: Option<String>,
    #[serde(with = "serde_millis")]
    pub(super) position: Option<Duration>,
    pub(super) speed: f64,
    pub(super) paused: bool,
}

impl From<NiketsuVideoStatus> for NiketsuMessage {
    fn from(value: NiketsuVideoStatus) -> Self {
        Self::VideoStatus(VideoStatusMessage {
            filename: value.filename,
            position: value.position,
            speed: value.speed,
            paused: value.paused,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StatusListMessage {
    pub(super) rooms: BTreeMap<String, BTreeSet<UserStatusMessage>>,
}

impl From<StatusListMessage> for IncomingMessage {
    fn from(value: StatusListMessage) -> Self {
        let rooms: BTreeMap<String, BTreeSet<NiketsuUserStatus>> = value
            .rooms
            .into_iter()
            .map(|(name, room)| {
                (
                    name,
                    room.into_iter().map(NiketsuUserStatus::from).collect(),
                )
            })
            .collect();
        NiketsuUserStatusList { rooms }.into()
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PauseMessage {
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuPause> for NiketsuMessage {
    fn from(_: NiketsuPause) -> Self {
        Self::Pause(PauseMessage::default())
    }
}

impl From<PauseMessage> for IncomingMessage {
    fn from(value: PauseMessage) -> Self {
        NiketsuPause {
            actor: value.username,
        }
        .into()
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartMessage {
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuStart> for NiketsuMessage {
    fn from(_: NiketsuStart) -> Self {
        Self::Start(StartMessage::default())
    }
}

impl From<StartMessage> for IncomingMessage {
    fn from(value: StartMessage) -> Self {
        NiketsuStart {
            actor: value.username,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaybackSpeedMessage {
    pub(super) speed: f64,
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuPlaybackSpeed> for NiketsuMessage {
    fn from(value: NiketsuPlaybackSpeed) -> Self {
        Self::PlaybackSpeed(PlaybackSpeedMessage {
            speed: value.speed,
            username: Default::default(),
        })
    }
}

impl From<PlaybackSpeedMessage> for IncomingMessage {
    fn from(value: PlaybackSpeedMessage) -> Self {
        NiketsuPlaybackSpeed {
            actor: value.username,
            speed: value.speed,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SeekMessage {
    pub(super) filename: String,
    #[serde(with = "serde_millis")]
    pub(super) position: Duration,
    #[serde(skip_serializing)]
    pub(super) username: String,
    pub(super) paused: bool,
    pub(super) speed: f64,
    #[serde(skip_serializing)]
    pub(super) desync: bool,
}

impl From<NiketsuSeek> for NiketsuMessage {
    fn from(value: NiketsuSeek) -> Self {
        Self::Seek(SeekMessage {
            filename: value.file,
            position: value.position,
            username: Default::default(),
            paused: value.paused,
            speed: value.speed,
            desync: false,
        })
    }
}

impl From<SeekMessage> for IncomingMessage {
    fn from(value: SeekMessage) -> Self {
        NiketsuSeek {
            position: value.position,
            paused: value.paused,
            speed: value.speed,
            actor: value.username,
            file: value.filename,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SelectMessage {
    pub(super) filename: Option<String>,
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuSelect> for NiketsuMessage {
    fn from(value: NiketsuSelect) -> Self {
        Self::Select(SelectMessage {
            filename: value.filename,
            username: Default::default(),
        })
    }
}

impl From<SelectMessage> for IncomingMessage {
    fn from(value: SelectMessage) -> Self {
        NiketsuSelect {
            actor: value.username,
            filename: value.filename,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UserMessageMessage {
    pub(super) message: String,
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuUserMessage> for NiketsuMessage {
    fn from(value: NiketsuUserMessage) -> Self {
        Self::UserMessage(UserMessageMessage {
            message: value.message,
            username: Default::default(),
        })
    }
}

impl From<UserMessageMessage> for IncomingMessage {
    fn from(value: UserMessageMessage) -> Self {
        NiketsuUserMessage {
            actor: value.username,
            message: value.message,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ServerMessageMessage {
    pub(super) message: String,
    pub(super) error: bool,
}

impl From<ServerMessageMessage> for IncomingMessage {
    fn from(value: ServerMessageMessage) -> Self {
        NiketsuServerMessage {
            message: value.message,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaylistMessage {
    pub(super) playlist: Vec<String>,
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuPlaylist> for NiketsuMessage {
    fn from(value: NiketsuPlaylist) -> Self {
        Self::Playlist(PlaylistMessage {
            playlist: value.playlist,
            username: Default::default(),
        })
    }
}

impl From<PlaylistMessage> for IncomingMessage {
    fn from(value: PlaylistMessage) -> Self {
        NiketsuPlaylist {
            actor: value.username,
            playlist: value.playlist,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct UserStatusMessage {
    pub(super) username: String,
    pub(super) ready: bool,
}

impl From<NiketsuUserStatus> for NiketsuMessage {
    fn from(value: NiketsuUserStatus) -> Self {
        Self::Status(UserStatusMessage {
            username: value.username,
            ready: value.ready,
        })
    }
}

impl From<UserStatusMessage> for NiketsuUserStatus {
    fn from(value: UserStatusMessage) -> Self {
        Self {
            ready: value.ready,
            username: value.username,
        }
    }
}

impl PartialEq for UserStatusMessage {
    fn eq(&self, other: &Self) -> bool {
        self.username.eq(&other.username)
    }
}

impl Ord for UserStatusMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.username.cmp(&other.username)
    }
}
impl PartialOrd for UserStatusMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.username.partial_cmp(&other.username)
    }
}

impl From<OutgoingMessage> for NiketsuMessage {
    fn from(value: OutgoingMessage) -> Self {
        match value {
            OutgoingMessage::Join(msg) => msg.into(),
            OutgoingMessage::VideoStatus(msg) => msg.into(),
            OutgoingMessage::Start(msg) => msg.into(),
            OutgoingMessage::Pause(msg) => msg.into(),
            OutgoingMessage::PlaybackSpeed(msg) => msg.into(),
            OutgoingMessage::Seek(msg) => msg.into(),
            OutgoingMessage::Select(msg) => msg.into(),
            OutgoingMessage::UserMessage(msg) => msg.into(),
            OutgoingMessage::Playlist(msg) => msg.into(),
            OutgoingMessage::UserStatus(msg) => msg.into(),
        }
    }
}
