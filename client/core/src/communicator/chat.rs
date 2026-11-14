//! How a message reads as a line in the chat.

use arcstr::ArcStr;

use super::messages::*;
use crate::ui::{MessageLevel, MessageSource, PlayerMessage};

/// A line about something `actor` did in the room.
fn action(actor: ArcStr, message: String) -> PlayerMessage {
    PlayerMessage::new(
        message,
        MessageSource::UserAction(actor),
        MessageLevel::Normal,
    )
}

impl From<ConnectedMsg> for PlayerMessage {
    fn from(_: ConnectedMsg) -> Self {
        PlayerMessage::new(
            "connected to the room".to_string(),
            MessageSource::Internal,
            MessageLevel::Normal,
        )
    }
}

impl IntentMsg {
    /// The line telling the room what the host did for this intent. A
    /// user's status gets none, the user list shows it.
    pub(crate) fn chat_line(&self) -> Option<PlayerMessage> {
        let actor = &self.actor;
        let message = match &self.intent {
            Intent::Pause => format!("{actor} paused playback"),
            Intent::Start => format!("{actor} started playback"),
            Intent::Speed(speed) => format!("{actor} changed playback speed to {speed}"),
            Intent::Seek { position, .. } => format!("{actor} seeked to {position:?}"),
            Intent::Select {
                video: Some(video), ..
            } => format!("{actor} selected {}", video.as_str()),
            Intent::Select { video: None, .. } => format!("{actor} unselected video"),
            Intent::Playlist(_) => format!("{actor} changed playlist"),
            Intent::Status(_) => return None,
        };
        Some(action(actor.clone(), message))
    }
}

impl From<UserMessageMsg> for PlayerMessage {
    fn from(value: UserMessageMsg) -> Self {
        PlayerMessage::new(
            value.message,
            MessageSource::UserMessage(value.actor),
            MessageLevel::Normal,
        )
    }
}

impl From<NoticeMsg> for PlayerMessage {
    fn from(value: NoticeMsg) -> Self {
        PlayerMessage::new(value.message, MessageSource::Room, MessageLevel::Normal)
    }
}

impl From<ForcedRenameMsg> for PlayerMessage {
    fn from(value: ForcedRenameMsg) -> Self {
        let name = value.name;
        PlayerMessage::new(
            format!("Username changed to {name}"),
            MessageSource::Room,
            MessageLevel::Warn,
        )
    }
}

impl From<FileRequestMsg> for PlayerMessage {
    fn from(value: FileRequestMsg) -> Self {
        let actor = value.actor;
        let video = value.video;
        let message = format!("Received video request for {video:?} for {actor:?}");
        action(actor, message)
    }
}

impl From<FileResponseMsg> for PlayerMessage {
    fn from(value: FileResponseMsg) -> Self {
        let actor = value.actor;
        let video = value.video;
        PlayerMessage::new(
            format!("Received video response for {video:?} from {actor:?}"),
            MessageSource::UserAction(actor),
            MessageLevel::Debug,
        )
    }
}
