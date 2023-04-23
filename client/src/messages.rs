use std::time::Duration;

use chrono::{DateTime, Local};
use iced::widget::Text;
use iced::{Color, Length, Renderer, Theme};

#[derive(Debug, Clone)]
pub enum ChatMessage {
    PlaylistChanged {
        when: DateTime<Local>,
        user: String,
    },
    Paused {
        when: DateTime<Local>,
        user: String,
    },
    Started {
        when: DateTime<Local>,
        user: String,
    },
    Select {
        when: DateTime<Local>,
        user: String,
        file: Option<String>,
    },
    Seek {
        when: DateTime<Local>,
        user: String,
        file: String,
        pos: Duration,
    },
    Chat {
        when: DateTime<Local>,
        user: String,
        msg: String,
    },
    Connected {
        when: DateTime<Local>,
    },
    Disconnected {
        when: DateTime<Local>,
    },
}

impl ChatMessage {
    pub fn playlist_changed(user: String) -> Self {
        Self::PlaylistChanged {
            when: Local::now(),
            user,
        }
    }

    pub fn paused(user: String) -> Self {
        Self::Paused {
            when: Local::now(),
            user,
        }
    }

    pub fn started(user: String) -> Self {
        Self::Started {
            when: Local::now(),
            user,
        }
    }

    pub fn select(file: Option<String>, user: String) -> Self {
        Self::Select {
            when: Local::now(),
            user,
            file,
        }
    }

    pub fn seek(pos: Duration, file: String, user: String) -> Self {
        Self::Seek {
            when: Local::now(),
            user,
            file,
            pos,
        }
    }

    pub fn chat(msg: String, user: String) -> Self {
        Self::Chat {
            when: Local::now(),
            user,
            msg,
        }
    }

    pub fn connected() -> Self {
        Self::Connected { when: Local::now() }
    }

    pub fn disconnected() -> Self {
        Self::Disconnected { when: Local::now() }
    }

    pub fn to_text<'a>(&self, theme: Theme) -> Text<'a, Renderer> {
        let when = self.when().format("[%H:%M:%S]").to_string();
        let style = self.style(theme);

        match self {
            ChatMessage::PlaylistChanged { user, .. } => {
                Text::new(format!("{when} {user} changed playlist")).style(style)
            }
            ChatMessage::Paused { user, .. } => {
                Text::new(format!("{when} {user} paused playback")).style(style)
            }
            ChatMessage::Started { user, .. } => {
                Text::new(format!("{when} {user} started playback")).style(style)
            }
            ChatMessage::Select { user, file, .. } => {
                Text::new(format!("{when} {user} selected file: {file:?}")).style(style)
            }
            ChatMessage::Chat { user, msg, .. } => Text::new(format!("{when} {user}: {msg}")),
            ChatMessage::Disconnected { .. } => {
                Text::new(format!("{when} lost connection to server")).style(style)
            }
            ChatMessage::Connected { .. } => {
                Text::new(format!("{when} connection to server established")).style(style)
            }
            ChatMessage::Seek { user, pos, .. } => {
                Text::new(format!("{when} {user} seeked to {pos:?}")).style(style)
            }
        }
        .width(Length::Fill)
    }

    pub fn style(&self, theme: Theme) -> Color {
        match self {
            ChatMessage::PlaylistChanged { .. } => theme.palette().primary,
            ChatMessage::Paused { .. } => theme.palette().primary,
            ChatMessage::Started { .. } => theme.palette().primary,
            ChatMessage::Select { .. } => theme.palette().primary,
            ChatMessage::Chat { .. } => theme.palette().text,
            ChatMessage::Connected { .. } => theme.palette().success,
            ChatMessage::Disconnected { .. } => theme.palette().danger,
            ChatMessage::Seek { .. } => theme.palette().primary,
        }
    }

    pub fn when(&self) -> &DateTime<Local> {
        match self {
            ChatMessage::PlaylistChanged { when, .. } => when,
            ChatMessage::Paused { when, .. } => when,
            ChatMessage::Started { when, .. } => when,
            ChatMessage::Select { when, .. } => when,
            ChatMessage::Chat { when, .. } => when,
            ChatMessage::Disconnected { when } => when,
            ChatMessage::Connected { when } => when,
            ChatMessage::Seek { when, .. } => when,
        }
    }
}
