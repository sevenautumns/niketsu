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
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    ChunkRequest(ChunkRequestMsg),
    ChunkResponse(ChunkResponseMsg),
    VideoShare(VideoShareMsg),
    VideoProviderStopped(VideoProviderStoppedMsg),
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
            NiketsuMessage::FileResponse(m) => Ok(m.into()),
            NiketsuMessage::FileRequest(m) => Ok(m.into()),
            NiketsuMessage::ChunkResponse(m) => Ok(m.into()),
            NiketsuMessage::ChunkRequest(m) => Ok(m.into()),
            NiketsuMessage::VideoProviderStopped(m) => Ok(m.into()),
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

impl From<ConnectedMsg> for NiketsuMessage {
    fn from(value: ConnectedMsg) -> Self {
        Self::Connection(value)
    }
}

impl From<ChunkRequestMsg> for NiketsuMessage {
    fn from(value: ChunkRequestMsg) -> Self {
        Self::ChunkRequest(value)
    }
}

impl From<ChunkResponseMsg> for NiketsuMessage {
    fn from(value: ChunkResponseMsg) -> Self {
        Self::ChunkResponse(value)
    }
}

impl From<FileResponseMsg> for NiketsuMessage {
    fn from(value: FileResponseMsg) -> Self {
        Self::FileResponse(value)
    }
}

impl From<FileRequestMsg> for NiketsuMessage {
    fn from(value: FileRequestMsg) -> Self {
        Self::FileRequest(value)
    }
}

impl From<VideoShareMsg> for NiketsuMessage {
    fn from(value: VideoShareMsg) -> Self {
        Self::VideoShare(value)
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
            OutgoingMessage::FileRequest(msg) => msg.into(),
            OutgoingMessage::FileResponse(msg) => msg.into(),
            OutgoingMessage::ChunkRequest(msg) => msg.into(),
            OutgoingMessage::ChunkResponse(msg) => msg.into(),
            OutgoingMessage::VideoShareChange(msg) => msg.into(),
        }
    }
}
