//! How a message reads as a line in the chat.

use arcstr::ArcStr;
use chrono::Local;

use super::messages::*;
use crate::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use crate::user::UserStatus;

fn line(message: String, source: MessageSource, level: MessageLevel) -> PlayerMessage {
    PlayerMessageInner {
        message,
        source,
        level,
        timestamp: Local::now(),
    }
    .into()
}

/// A line about something `actor` did in the room.
fn action(actor: ArcStr, message: String) -> PlayerMessage {
    line(
        message,
        MessageSource::UserAction(actor),
        MessageLevel::Normal,
    )
}

impl From<ConnectedMsg> for PlayerMessage {
    fn from(_: ConnectedMsg) -> Self {
        line(
            "connected to the room".to_string(),
            MessageSource::Internal,
            MessageLevel::Normal,
        )
    }
}

impl From<StartMsg> for PlayerMessage {
    fn from(value: StartMsg) -> Self {
        let actor = value.actor;
        let message = format!("{actor} started playback");
        action(actor, message)
    }
}

impl From<PauseMsg> for PlayerMessage {
    fn from(value: PauseMsg) -> Self {
        let actor = value.actor;
        let message = format!("{actor} paused playback");
        action(actor, message)
    }
}

impl From<PlaybackSpeedMsg> for PlayerMessage {
    fn from(value: PlaybackSpeedMsg) -> Self {
        let actor = value.actor;
        let speed = value.speed;
        let message = format!("{actor} changed playback speed to {speed}");
        action(actor, message)
    }
}

impl From<SeekMsg> for PlayerMessage {
    fn from(value: SeekMsg) -> Self {
        let actor = value.actor;
        let position = value.position;
        let message = format!("{actor} seeked to {position:?}");
        action(actor, message)
    }
}

impl From<SelectMsg> for PlayerMessage {
    fn from(value: SelectMsg) -> Self {
        let actor = value.actor;
        let message = if let Some(video) = value.video {
            format!("{actor} selected {}", video.as_str())
        } else {
            format!("{actor} unselected video")
        };
        action(actor, message)
    }
}

impl From<UserMessageMsg> for PlayerMessage {
    fn from(value: UserMessageMsg) -> Self {
        line(
            value.message,
            MessageSource::UserMessage(value.actor),
            MessageLevel::Normal,
        )
    }
}

impl From<NoticeMsg> for PlayerMessage {
    fn from(value: NoticeMsg) -> Self {
        line(value.message, MessageSource::Room, MessageLevel::Normal)
    }
}

impl From<PlaylistMsg> for PlayerMessage {
    fn from(value: PlaylistMsg) -> Self {
        let actor = value.actor;
        let message = format!("{actor} changed playlist");
        action(actor, message)
    }
}

/// A rename the host forced on this user.
impl From<UserStatus> for PlayerMessage {
    fn from(value: UserStatus) -> Self {
        let name = value.name;
        line(
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
        line(
            format!("Received video response for {video:?} from {actor:?}"),
            MessageSource::UserAction(actor),
            MessageLevel::Debug,
        )
    }
}

impl From<HostHandoverMsg> for PlayerMessage {
    fn from(value: HostHandoverMsg) -> Self {
        line(
            format!("{} is now the host", value.new_host),
            MessageSource::Internal,
            MessageLevel::Success,
        )
    }
}
