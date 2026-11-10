use anyhow::Context;
use niketsu_core::communicator::*;
use serde::{Deserialize, Serialize};

/// The room messages that travel between peers, over gossipsub or as a
/// direct request. File sharing has its own protocol (see `p2p/file_share`).
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub(super) enum NiketsuMessage {
    VideoStatus(VideoStatusMsg),
    StatusList(UserStatusListMsg),
    Pause(PauseMsg),
    Start(StartMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    Playlist(PlaylistMsg),
    Status(UserStatusMsg),
    HostHandover(HostHandoverMsg),
}

impl From<NiketsuMessage> for IncomingMessage {
    fn from(value: NiketsuMessage) -> Self {
        match value {
            NiketsuMessage::VideoStatus(m) => m.into(),
            NiketsuMessage::StatusList(m) => m.into(),
            NiketsuMessage::Pause(m) => m.into(),
            NiketsuMessage::Start(m) => m.into(),
            NiketsuMessage::PlaybackSpeed(m) => m.into(),
            NiketsuMessage::Seek(m) => m.into(),
            NiketsuMessage::Select(m) => m.into(),
            NiketsuMessage::UserMessage(m) => m.into(),
            NiketsuMessage::Playlist(m) => m.into(),
            NiketsuMessage::Status(m) => m.into(),
            NiketsuMessage::HostHandover(m) => m.into(),
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

impl From<VideoStatusMsg> for NiketsuMessage {
    fn from(value: VideoStatusMsg) -> Self {
        Self::VideoStatus(value)
    }
}

impl From<UserStatusListMsg> for NiketsuMessage {
    fn from(value: UserStatusListMsg) -> Self {
        Self::StatusList(value)
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

impl From<HostHandoverMsg> for NiketsuMessage {
    fn from(value: HostHandoverMsg) -> Self {
        Self::HostHandover(value)
    }
}

impl From<DirectMessage> for NiketsuMessage {
    fn from(value: DirectMessage) -> Self {
        match value {
            DirectMessage::Playlist(msg) => msg.into(),
            DirectMessage::Select(msg) => msg.into(),
            DirectMessage::UserStatus(msg) => msg.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn roundtrip(msg: NiketsuMessage) -> NiketsuMessage {
        let bytes: Vec<u8> = msg.try_into().unwrap();
        bytes.try_into().unwrap()
    }

    #[test]
    fn room_messages_survive_the_wire() {
        let messages = [
            NiketsuMessage::Select(SelectMsg {
                actor: "alice".into(),
                position: Duration::from_millis(1500),
                video: Some("ep1".into()),
            }),
            NiketsuMessage::Status(UserStatusMsg {
                name: "bob_2".into(),
                ready: true,
            }),
            NiketsuMessage::HostHandover(HostHandoverMsg {
                new_host: "bob".into(),
            }),
        ];
        for msg in messages {
            assert_eq!(roundtrip(msg.clone()), msg);
        }
    }

    #[test]
    fn the_wire_tag_names_the_message() {
        let msg = NiketsuMessage::Pause(PauseMsg {
            actor: "alice".into(),
        });
        let bytes: Vec<u8> = msg.try_into().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["type"], "pause");
        assert_eq!(json["actor"], "alice");
    }
}
