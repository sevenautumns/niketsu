//! How a message reads as a line in the chat.

use arcstr::ArcStr;
use multiaddr::PeerId;

use super::messages::*;
use crate::ui::{MessageLevel, MessageSource, PlayerMessage};

impl From<ConnectedMsg> for PlayerMessage {
    fn from(_: ConnectedMsg) -> Self {
        PlayerMessage::new(
            "connected to the room".to_string(),
            MessageSource::Internal,
            MessageLevel::Normal,
        )
    }
}

impl Intent {
    /// The line telling the room what the host did for this intent of
    /// `actor`, the user at `peer`. A user's status gets none, the user
    /// list shows it.
    pub(crate) fn line(&self, actor: &ArcStr, peer: PeerId) -> Option<LineMsg> {
        let message = match self {
            Intent::Pause | Intent::CachePause => format!("{actor} paused playback"),
            Intent::Start => format!("{actor} started playback"),
            Intent::Speed(speed) => format!("{actor} changed playback speed to {speed}"),
            Intent::Seek { position, .. } => format!("{actor} seeked to {position:?}"),
            Intent::Select {
                video: Some(video), ..
            } => format!("{actor} selected {}", video.as_str()),
            Intent::Select { video: None, .. } => format!("{actor} unselected video"),
            Intent::Playlist(_) => format!("{actor} changed playlist"),
            Intent::Chat(message) => {
                return Some(LineMsg {
                    actor: actor.clone(),
                    message: message.clone(),
                    kind: LineKind::Chat { author: peer },
                });
            }
            // the line of an ended video is that of the selection after it
            Intent::Status(_) | Intent::Ended(_) => return None,
        };
        Some(LineMsg {
            actor: actor.clone(),
            message,
            kind: LineKind::Action,
        })
    }
}

impl From<LineMsg> for PlayerMessage {
    fn from(value: LineMsg) -> Self {
        let source = match value.kind {
            LineKind::Action => MessageSource::UserAction(value.actor),
            LineKind::Chat { .. } => MessageSource::UserMessage(value.actor),
        };
        PlayerMessage::new(value.message, source, MessageLevel::Normal)
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

impl PlayerMessage {
    /// The line telling this core's user that the room knows them by
    /// `name`, which another user went by when they asked for theirs.
    pub(crate) fn renamed(name: &ArcStr) -> Self {
        PlayerMessage::new(
            format!("Username changed to {name}"),
            MessageSource::Room,
            MessageLevel::Warn,
        )
    }
}
