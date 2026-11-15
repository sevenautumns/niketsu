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
    pub(crate) fn action(&self) -> Option<ActionMsg> {
        let actor = &self.actor;
        let message = match &self.intent {
            Intent::Pause | Intent::CachePause => format!("{actor} paused playback"),
            Intent::Start => format!("{actor} started playback"),
            Intent::Speed(speed) => format!("{actor} changed playback speed to {speed}"),
            Intent::Seek { position, .. } => format!("{actor} seeked to {position:?}"),
            Intent::Select {
                video: Some(video), ..
            } => format!("{actor} selected {}", video.as_str()),
            Intent::Select { video: None, .. } => format!("{actor} unselected video"),
            Intent::Playlist(_) => format!("{actor} changed playlist"),
            // the line of an ended video is that of the selection after it
            Intent::Status(_) | Intent::Ended(_) => return None,
        };
        Some(ActionMsg {
            actor: actor.clone(),
            message,
        })
    }
}

impl From<ActionMsg> for PlayerMessage {
    fn from(value: ActionMsg) -> Self {
        action(value.actor, value.message)
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

impl From<DisconnectedMsg> for PlayerMessage {
    fn from(value: DisconnectedMsg) -> Self {
        PlayerMessage::new(value.reason, MessageSource::Room, MessageLevel::Normal)
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
