use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use arcstr::ArcStr;
use im::Vector;
use niketsu_core::communicator::*;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
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
            NiketsuMessage::Seek(m) => Ok(m.into()),
            NiketsuMessage::Select(m) => Ok(m.into()),
            NiketsuMessage::UserMessage(m) => Ok(m.into()),
            NiketsuMessage::ServerMessage(m) => Ok(m.into()),
            NiketsuMessage::Playlist(m) => Ok(m.into()),
            NiketsuMessage::Status(m) => Ok(m.into()),
            value => Err(value),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PingMessage {
    uuid: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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
    pub(super) file_loaded: bool,
}

impl PartialEq for VideoStatusMessage {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        self.filename.eq(&other.filename)
            && self.position.eq(&other.position)
            && speed_self.eq(&speed_other)
            && self.paused.eq(&other.paused)
    }
}

impl Eq for VideoStatusMessage {}

impl From<NiketsuVideoStatus> for NiketsuMessage {
    fn from(value: NiketsuVideoStatus) -> Self {
        Self::VideoStatus(VideoStatusMessage {
            filename: value.filename,
            position: value.position,
            speed: value.speed,
            paused: value.paused,
            file_loaded: value.file_loaded,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

impl PartialEq for PlaybackSpeedMessage {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        speed_self.eq(&speed_other) && self.username.eq(&other.username)
    }
}

impl Eq for PlaybackSpeedMessage {}

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct SeekMessage {
    pub(super) filename: String,
    #[serde(with = "serde_millis")]
    pub(super) position: Duration,
    #[serde(skip_serializing)]
    pub(super) username: String,
    #[serde(skip_serializing)]
    pub(super) desync: bool,
}

impl From<NiketsuSeek> for NiketsuMessage {
    fn from(value: NiketsuSeek) -> Self {
        Self::Seek(SeekMessage {
            filename: value.file,
            position: value.position,
            username: Default::default(),
            desync: false,
        })
    }
}

impl From<SeekMessage> for IncomingMessage {
    fn from(value: SeekMessage) -> Self {
        NiketsuSeek {
            position: value.position,
            actor: value.username,
            file: value.filename,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct SelectMessage {
    pub(super) filename: Option<String>,
    #[serde(with = "serde_millis")]
    pub(super) position: Duration,
    #[serde(skip_serializing)]
    pub(super) username: String,
}

impl From<NiketsuSelect> for NiketsuMessage {
    fn from(value: NiketsuSelect) -> Self {
        Self::Select(SelectMessage {
            filename: value.filename,
            username: Default::default(),
            position: value.position,
        })
    }
}

impl From<SelectMessage> for IncomingMessage {
    fn from(value: SelectMessage) -> Self {
        NiketsuSelect {
            actor: value.username,
            filename: value.filename,
            position: value.position,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlaylistMessage {
    pub(super) playlist: Vector<ArcStr>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

impl From<UserStatusMessage> for IncomingMessage {
    fn from(value: UserStatusMessage) -> Self {
        NiketsuUserStatus {
            ready: value.ready,
            username: value.username,
        }
        .into()
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::messages::*;

    #[test]
    fn test_ping_message_serialization() {
        let ping_message = NiketsuMessage::Ping(PingMessage {
            uuid: String::from("some_uuid"),
        });

        let json_str = serde_json::to_string(&ping_message).unwrap();
        let expected_json = r#"{"type":"ping","uuid":"some_uuid"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_join_message_serialization() {
        let join_message = NiketsuMessage::Join(JoinMessage {
            password: String::from("password"),
            room: String::from("room"),
            username: String::from("username"),
        });

        let json_str = serde_json::to_string(&join_message).unwrap();
        let expected_json =
            r#"{"type":"join","password":"password","room":"room","username":"username"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_video_status_message_serialization() {
        let video_status_message = NiketsuMessage::VideoStatus(VideoStatusMessage {
            filename: Some(String::from("video.mp4")),
            position: Some(Duration::from_secs(60)),
            speed: 1.0,
            paused: false,
            file_loaded: true,
        });

        let json_str = serde_json::to_string(&video_status_message).unwrap();
        let expected_json = r#"{"type":"videoStatus","filename":"video.mp4","position":60000,"speed":1.0,"paused":false,"fileLoaded":true}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_status_list_message_serialization() {
        let mut rooms = BTreeMap::new();
        let mut users = BTreeSet::new();
        users.insert(UserStatusMessage {
            username: String::from("user1"),
            ready: true,
        });
        users.insert(UserStatusMessage {
            username: String::from("user2"),
            ready: false,
        });
        rooms.insert(String::from("room1"), users);

        let status_list_message = NiketsuMessage::StatusList(StatusListMessage { rooms });

        let json_str = serde_json::to_string(&status_list_message).unwrap();
        let expected_json = r#"{"type":"statusList","rooms":{"room1":[{"username":"user1","ready":true},{"username":"user2","ready":false}]}}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_pause_message_serialization() {
        let pause_message = NiketsuMessage::Pause(PauseMessage {
            username: String::from("user1"),
        });

        let json_str = serde_json::to_string(&pause_message).unwrap();
        let expected_json = r#"{"type":"pause"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_start_message_serialization() {
        let start_message = NiketsuMessage::Start(StartMessage {
            username: String::from("user1"),
        });

        let json_str = serde_json::to_string(&start_message).unwrap();
        let expected_json = r#"{"type":"start"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_playback_speed_message_serialization() {
        let playback_speed_message = NiketsuMessage::PlaybackSpeed(PlaybackSpeedMessage {
            speed: 1.5,
            username: String::from("user1"),
        });

        let json_str = serde_json::to_string(&playback_speed_message).unwrap();
        let expected_json = r#"{"type":"playbackSpeed","speed":1.5}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_seek_message_serialization() {
        let seek_message = NiketsuMessage::Seek(SeekMessage {
            filename: String::from("video.mp4"),
            position: Duration::from_secs(120),
            username: String::from("user1"),
            desync: false,
        });

        let json_str = serde_json::to_string(&seek_message).unwrap();
        let expected_json = r#"{"type":"seek","filename":"video.mp4","position":120000}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_select_message_serialization() {
        let select_message = NiketsuMessage::Select(SelectMessage {
            filename: Some(String::from("video.mp4")),
            username: String::from("user1"),
            position: Duration::from_secs(60),
        });

        let json_str = serde_json::to_string(&select_message).unwrap();
        let expected_json = r#"{"type":"select","filename":"video.mp4","position":60000}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_user_message_message_serialization() {
        let user_message_message = NiketsuMessage::UserMessage(UserMessageMessage {
            message: String::from("Hello, world!"),
            username: String::from("user1"),
        });

        let json_str = serde_json::to_string(&user_message_message).unwrap();
        let expected_json = r#"{"type":"userMessage","message":"Hello, world!"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_server_message_message_serialization() {
        let server_message_message = NiketsuMessage::ServerMessage(ServerMessageMessage {
            message: String::from("Server error"),
            error: true,
        });

        let json_str = serde_json::to_string(&server_message_message).unwrap();
        let expected_json = r#"{"type":"serverMessage","message":"Server error","error":true}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_playlist_message_serialization() {
        let playlist_message = NiketsuMessage::Playlist(PlaylistMessage {
            playlist: Vector::from(vec![ArcStr::from("song1"), ArcStr::from("song2")]),
            username: String::from("user1"),
        });

        let json_str = serde_json::to_string(&playlist_message).unwrap();
        let expected_json = r#"{"type":"playlist","playlist":["song1","song2"]}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_user_status_message_serialization() {
        let user_status_message = NiketsuMessage::Status(UserStatusMessage {
            username: String::from("user1"),
            ready: true,
        });

        let json_str = serde_json::to_string(&user_status_message).unwrap();
        let expected_json = r#"{"type":"status","username":"user1","ready":true}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_ping_message_deserialization() {
        let json_str = r#"{"type":"ping","uuid":"some_uuid"}"#;
        let expected_ping_message = NiketsuMessage::Ping(PingMessage {
            uuid: String::from("some_uuid"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_ping_message);
    }

    #[test]
    fn test_join_message_deserialization() {
        let json_str =
            r#"{"type":"join","password":"password","room":"room","username":"username"}"#;
        let expected_join_message = NiketsuMessage::Join(JoinMessage {
            password: String::from("password"),
            room: String::from("room"),
            username: String::from("username"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_join_message);
    }

    #[test]
    fn test_video_status_message_deserialization() {
        let json_str = r#"{"type":"videoStatus","filename":"video.mp4","position":60000,"speed":1,"paused":false, "fileLoaded":true}"#;
        let expected_video_status_message = NiketsuMessage::VideoStatus(VideoStatusMessage {
            filename: Some(String::from("video.mp4")),
            position: Some(Duration::from_secs(60)),
            speed: 1.0,
            paused: false,
            file_loaded: true,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_video_status_message);
    }

    #[test]
    fn test_status_list_message_deserialization() {
        let json_str = r#"{"type":"statusList","rooms":{"room1":[{"username":"user1","ready":true},{"username":"user2","ready":false}]}}"#;
        let mut expected_rooms = BTreeMap::new();
        let mut users = BTreeSet::new();
        users.insert(UserStatusMessage {
            username: String::from("user1"),
            ready: true,
        });
        users.insert(UserStatusMessage {
            username: String::from("user2"),
            ready: false,
        });
        expected_rooms.insert(String::from("room1"), users);

        let expected_status_list_message = NiketsuMessage::StatusList(StatusListMessage {
            rooms: expected_rooms,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_status_list_message);
    }

    #[test]
    fn test_pause_message_deserialization() {
        let json_str = r#"{"type":"pause","username":"user1"}"#;
        let expected_pause_message = NiketsuMessage::Pause(PauseMessage {
            username: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_pause_message);
    }

    #[test]
    fn test_start_message_deserialization() {
        let json_str = r#"{"type":"start","username":"user1"}"#;
        let expected_start_message = NiketsuMessage::Start(StartMessage {
            username: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_start_message);
    }

    #[test]
    fn test_playback_speed_message_deserialization() {
        let json_str = r#"{"type":"playbackSpeed","speed":1.5,"username":"user1"}"#;
        let expected_playback_speed_message = NiketsuMessage::PlaybackSpeed(PlaybackSpeedMessage {
            speed: 1.5,
            username: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_playback_speed_message);
    }

    #[test]
    fn test_seek_message_deserialization() {
        let json_str = r#"{"type":"seek","filename":"video.mp4","position":120000,"username":"user1","desync":false}"#;
        let expected_seek_message = NiketsuMessage::Seek(SeekMessage {
            filename: String::from("video.mp4"),
            position: Duration::from_secs(120),
            username: String::from("user1"),
            desync: false,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_seek_message);
    }

    #[test]
    fn test_select_message_deserialization() {
        let json_str =
            r#"{"type":"select","filename":"video.mp4","position":60000,"username":"user1"}"#;
        let expected_select_message = NiketsuMessage::Select(SelectMessage {
            filename: Some(String::from("video.mp4")),
            username: String::from("user1"),
            position: Duration::from_secs(60),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_select_message);
    }

    #[test]
    fn test_user_message_message_deserialization() {
        let json_str = r#"{"type":"userMessage","message":"Hello, world!","username":"user1"}"#;
        let expected_user_message_message = NiketsuMessage::UserMessage(UserMessageMessage {
            message: String::from("Hello, world!"),
            username: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_user_message_message);
    }

    #[test]
    fn test_server_message_message_deserialization() {
        let json_str = r#"{"type":"serverMessage","message":"Server error","error":true}"#;
        let expected_server_message_message = NiketsuMessage::ServerMessage(ServerMessageMessage {
            message: String::from("Server error"),
            error: true,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_server_message_message);
    }

    #[test]
    fn test_playlist_message_deserialization() {
        let json_str = r#"{"type":"playlist","playlist":["song1","song2"],"username":"user1"}"#;
        let expected_playlist_message = NiketsuMessage::Playlist(PlaylistMessage {
            playlist: Vector::from(vec![ArcStr::from("song1"), ArcStr::from("song2")]),
            username: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_playlist_message);
    }

    #[test]
    fn test_user_status_message_deserialization() {
        let json_str = r#"{"type":"status","username":"user1","ready":true}"#;
        let expected_user_status_message = NiketsuMessage::Status(UserStatusMessage {
            username: String::from("user1"),
            ready: true,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_user_status_message);
    }
}
