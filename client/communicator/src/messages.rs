use anyhow::Context;
use niketsu_core::communicator::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    Join(JoinMessage),
    VideoStatus(VideoStatusMsg),
    StatusList(UserStatusListMsg),
    Pause(PauseMsg),
    Start(StartMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    ServerMessage(ServerMessageMsg),
    Playlist(PlaylistMsg),
    Status(UserStatusMsg),
    Connection(ConnectedMsg),
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
            NiketsuMessage::Connection(m) => Ok(m.into()),
            NiketsuMessage::VideoStatus(m) => Ok(m.into()),
            value => Err(value),
        }
    }
}

impl TryFrom<NiketsuMessage> for Vec<u8> {
    type Error = anyhow::Error;

    fn try_from(value: NiketsuMessage) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(&value).context("serde json from_vec failed")
    }
}

impl TryFrom<Vec<u8>> for NiketsuMessage {
    type Error = anyhow::Error;

    fn try_from(value: Vec<u8>) -> anyhow::Result<NiketsuMessage> {
        let msg = std::str::from_utf8(&value).context("from utf8 failed")?;
        serde_json::from_str::<NiketsuMessage>(msg).context("serde json from_str failed")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct JoinMessage {
    pub(super) password: String,
    pub(super) room: String,
    pub(super) username: String,
}

impl From<VideoStatusMsg> for NiketsuMessage {
    fn from(value: VideoStatusMsg) -> Self {
        Self::VideoStatus(value)
    }
}

impl From<PauseMsg> for NiketsuMessage {
    fn from(value: PauseMsg) -> Self {
        Self::Pause(value)
    }
}

impl From<StartMsg> for NiketsuMessage {
    fn from(value: StartMsg) -> Self {
        Self::Start(value)
    }
}

impl From<PlaybackSpeedMsg> for NiketsuMessage {
    fn from(value: PlaybackSpeedMsg) -> Self {
        Self::PlaybackSpeed(value)
    }
}

impl From<SeekMsg> for NiketsuMessage {
    fn from(value: SeekMsg) -> Self {
        Self::Seek(value)
    }
}

impl From<SelectMsg> for NiketsuMessage {
    fn from(value: SelectMsg) -> Self {
        Self::Select(value)
    }
}

impl From<UserMessageMsg> for NiketsuMessage {
    fn from(value: UserMessageMsg) -> Self {
        Self::UserMessage(value)
    }
}

impl From<PlaylistMsg> for NiketsuMessage {
    fn from(value: PlaylistMsg) -> Self {
        Self::Playlist(value)
    }
}

impl From<UserStatusMsg> for NiketsuMessage {
    fn from(value: UserStatusMsg) -> Self {
        Self::Status(value)
    }
}

impl From<OutgoingMessage> for NiketsuMessage {
    fn from(value: OutgoingMessage) -> Self {
        match value {
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
    use std::collections::BTreeSet;
    use std::time::Duration;

    use niketsu_core::playlist::Playlist;
    use niketsu_core::room::RoomName;

    use crate::messages::*;

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
        let video_status_message = NiketsuMessage::VideoStatus(VideoStatusMsg {
            filename: Some(String::from("video.mp4")),
            position: Some(Duration::from_secs(60)),
            speed: 1.0,
            paused: false,
            file_loaded: true,
            cache: true,
        });

        let json_str = serde_json::to_string(&video_status_message).unwrap();
        let expected_json = r#"{"type":"videoStatus","filename":"video.mp4","position":60000,"speed":1.0,"paused":false,"fileLoaded":true,"cache":true}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_status_list_message_serialization() {
        let room_name = RoomName::from("room");
        let mut users = BTreeSet::new();
        users.insert(UserStatusMsg {
            name: String::from("user1"),
            ready: true,
        });
        users.insert(UserStatusMsg {
            name: String::from("user2"),
            ready: false,
        });
        let status_list_message =
            NiketsuMessage::StatusList(UserStatusListMsg { room_name, users });

        let json_str = serde_json::to_string(&status_list_message).unwrap();
        let expected_json = r#"{"type":"statusList","roomName":"room","users":[{"name":"user1","ready":true},{"name":"user2","ready":false}]}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_pause_message_serialization() {
        let pause_message = NiketsuMessage::Pause(PauseMsg {
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&pause_message).unwrap();
        let expected_json = r#"{"type":"pause","actor":"user1"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_start_message_serialization() {
        let start_message = NiketsuMessage::Start(StartMsg {
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&start_message).unwrap();
        let expected_json = r#"{"type":"start","actor":"user1"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_playback_speed_message_serialization() {
        let playback_speed_message = NiketsuMessage::PlaybackSpeed(PlaybackSpeedMsg {
            speed: 1.5,
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&playback_speed_message).unwrap();
        let expected_json = r#"{"type":"playbackSpeed","actor":"user1","speed":1.5}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_seek_message_serialization() {
        let seek_message = NiketsuMessage::Seek(SeekMsg {
            file: String::from("video.mp4"),
            position: Duration::from_secs(120),
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&seek_message).unwrap();
        let expected_json =
            r#"{"type":"seek","actor":"user1","file":"video.mp4","position":120000}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_select_message_serialization() {
        let select_message = NiketsuMessage::Select(SelectMsg {
            filename: Some(String::from("video.mp4")),
            actor: String::from("user1"),
            position: Duration::from_secs(60),
        });

        let json_str = serde_json::to_string(&select_message).unwrap();
        let expected_json =
            r#"{"type":"select","actor":"user1","position":60000,"filename":"video.mp4"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_user_message_message_serialization() {
        let user_message_message = NiketsuMessage::UserMessage(UserMessageMsg {
            message: String::from("Hello, world!"),
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&user_message_message).unwrap();
        let expected_json = r#"{"type":"userMessage","actor":"user1","message":"Hello, world!"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_server_message_message_serialization() {
        let server_message_message = NiketsuMessage::ServerMessage(ServerMessageMsg {
            message: String::from("Server error"),
        });

        let json_str = serde_json::to_string(&server_message_message).unwrap();
        let expected_json = r#"{"type":"serverMessage","message":"Server error"}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_playlist_message_serialization() {
        let playlist_message = NiketsuMessage::Playlist(PlaylistMsg {
            playlist: Playlist::from_iter(vec!["song1", "http://song2"]),
            actor: String::from("user1"),
        });

        let json_str = serde_json::to_string(&playlist_message).unwrap();
        let expected_json = r#"{"type":"playlist","actor":"user1","playlist":[{"File":"song1"},{"Url":"http://song2/"}]}"#;

        assert_eq!(json_str, expected_json);
    }

    #[test]
    fn test_user_status_message_serialization() {
        let user_status_message = NiketsuMessage::Status(UserStatusMsg {
            name: String::from("user1"),
            ready: true,
        });

        let json_str = serde_json::to_string(&user_status_message).unwrap();
        let expected_json = r#"{"type":"status","name":"user1","ready":true}"#;

        assert_eq!(json_str, expected_json);
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
        let json_str = r#"{"type":"videoStatus","filename":"video.mp4","position":60000,"speed":1,"paused":false, "fileLoaded":true,"cache":false}"#;
        let expected_video_status_message = NiketsuMessage::VideoStatus(VideoStatusMsg {
            filename: Some(String::from("video.mp4")),
            position: Some(Duration::from_secs(60)),
            speed: 1.0,
            paused: false,
            file_loaded: true,
            cache: false,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_video_status_message);
    }

    #[test]
    fn test_status_list_message_deserialization() {
        let json_str = r#"{"type":"statusList","roomName":"room","users":[{"name":"user1","ready":true},{"name":"user2","ready":false}]}"#;
        let mut users = BTreeSet::new();
        users.insert(UserStatusMsg {
            name: String::from("user1"),
            ready: true,
        });
        users.insert(UserStatusMsg {
            name: String::from("user2"),
            ready: false,
        });
        let expected_status_list_message = NiketsuMessage::StatusList(UserStatusListMsg {
            room_name: "room".into(),
            users,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_status_list_message);
    }

    #[test]
    fn test_pause_message_deserialization() {
        let json_str = r#"{"type":"pause","actor":"user1"}"#;
        let expected_pause_message = NiketsuMessage::Pause(PauseMsg {
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_pause_message);
    }

    #[test]
    fn test_start_message_deserialization() {
        let json_str = r#"{"type":"start","actor":"user1"}"#;
        let expected_start_message = NiketsuMessage::Start(StartMsg {
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_start_message);
    }

    #[test]
    fn test_playback_speed_message_deserialization() {
        let json_str = r#"{"type":"playbackSpeed","speed":1.5,"actor":"user1"}"#;
        let expected_playback_speed_message = NiketsuMessage::PlaybackSpeed(PlaybackSpeedMsg {
            speed: 1.5,
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_playback_speed_message);
    }

    #[test]
    fn test_seek_message_deserialization() {
        let json_str = r#"{"type":"seek","file":"video.mp4","position":120000,"actor":"user1","desync":false}"#;
        let expected_seek_message = NiketsuMessage::Seek(SeekMsg {
            file: String::from("video.mp4"),
            position: Duration::from_secs(120),
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_seek_message);
    }

    #[test]
    fn test_select_message_deserialization() {
        let json_str =
            r#"{"type":"select","filename":"video.mp4","position":60000,"actor":"user1"}"#;
        let expected_select_message = NiketsuMessage::Select(SelectMsg {
            filename: Some(String::from("video.mp4")),
            actor: String::from("user1"),
            position: Duration::from_secs(60),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_select_message);
    }

    #[test]
    fn test_user_message_message_deserialization() {
        let json_str = r#"{"type":"userMessage","message":"Hello, world!","actor":"user1"}"#;
        let expected_user_message_message = NiketsuMessage::UserMessage(UserMessageMsg {
            message: String::from("Hello, world!"),
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_user_message_message);
    }

    #[test]
    fn test_server_message_message_deserialization() {
        let json_str = r#"{"type":"serverMessage","message":"Server error","error":true}"#;
        let expected_server_message_message = NiketsuMessage::ServerMessage(ServerMessageMsg {
            message: String::from("Server error"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_server_message_message);
    }

    #[test]
    fn test_playlist_message_deserialization() {
        let json_str = r#"{"type":"playlist","actor":"user1","playlist":[{"File":"song1"},{"Url":"http://song2/"}]}"#;
        let expected_playlist_message = NiketsuMessage::Playlist(PlaylistMsg {
            playlist: Playlist::from_iter(vec!["song1", "http://song2"]),
            actor: String::from("user1"),
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_playlist_message);
    }

    #[test]
    fn test_user_status_message_deserialization() {
        let json_str = r#"{"type":"status","name":"user1","ready":true}"#;
        let expected_user_status_message = NiketsuMessage::Status(UserStatusMsg {
            name: String::from("user1"),
            ready: true,
        });

        let deserialized_message: NiketsuMessage = serde_json::from_str(json_str).unwrap();

        assert_eq!(deserialized_message, expected_user_status_message);
    }
}
