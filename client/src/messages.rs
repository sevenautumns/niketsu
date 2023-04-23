use std::time::Duration;

use chrono::{DateTime, Local};
use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{Column, Container, Scrollable, Text};
use iced::{Command, Element, Length, Renderer, Theme};

use crate::styling::{ContainerBackground, ContainerBorder};
use crate::window::MainMessage;

#[derive(Debug, Clone)]
pub struct Messages {
    scroll: RelativeOffset,
    msgs: Vec<ChatMessage>,
}

impl Default for Messages {
    fn default() -> Self {
        Self::new()
    }
}

impl Messages {
    pub fn new() -> Self {
        Self {
            msgs: vec![],
            scroll: RelativeOffset::END,
        }
    }

    pub fn set_offset(&mut self, offset: RelativeOffset) {
        self.scroll = offset;
    }

    pub fn push_playlist_changed(&mut self, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::PlaylistChanged {
            when: Local::now(),
            user,
        });
        self.snap_scroll()
    }

    pub fn push_paused(&mut self, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Paused {
            when: Local::now(),
            user,
        });
        self.snap_scroll()
    }

    pub fn push_started(&mut self, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Started {
            when: Local::now(),
            user,
        });
        self.snap_scroll()
    }

    pub fn push_select(&mut self, file: Option<String>, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Select {
            when: Local::now(),
            user,
            file,
        });
        self.snap_scroll()
    }

    pub fn push_seek(&mut self, pos: Duration, file: String, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Seek {
            when: Local::now(),
            user,
            file,
            pos,
        });
        self.snap_scroll()
    }

    pub fn push_chat(&mut self, msg: String, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Chat {
            when: Local::now(),
            user,
            msg,
        });
        self.snap_scroll()
    }

    pub fn push_connected(&mut self) -> Command<MainMessage> {
        self.msgs
            .push(ChatMessage::Connected { when: Local::now() });
        self.snap_scroll()
    }

    pub fn push_disconnected(&mut self) -> Command<MainMessage> {
        self.msgs
            .push(ChatMessage::Disconnected { when: Local::now() });
        self.snap_scroll()
    }

    fn snap_scroll(&self) -> Command<MainMessage> {
        if self.scroll.y.eq(&1.0) {
            return iced::widget::scrollable::snap_to(Id::new("messages"), RelativeOffset::END);
        }
        Command::none()
    }

    pub fn view<'a>(&self, theme: Theme) -> Element<'a, MainMessage, Renderer> {
        let msgs = self.msgs.iter().map(|m| m.to_text(theme.clone())).collect();
        Container::new(
            Scrollable::new(Column::with_children(msgs))
                .width(Length::Fill)
                .on_scroll(|o| MainMessage::User(crate::window::UserMessage::ScrollMessages(o)))
                .id(Id::new("messages")),
        )
        .style(ContainerBorder::basic())
        .padding(5.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

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
    pub fn to_text<'a>(&self, theme: Theme) -> Element<'a, MainMessage, Renderer> {
        let when = self.when().format("[%H:%M:%S]").to_string();
        let style = self.style(theme);

        let text = match self {
            ChatMessage::PlaylistChanged { user, .. } => {
                format!("{when} {user} changed playlist")
            }
            ChatMessage::Paused { user, .. } => {
                format!("{when} {user} paused playback")
            }
            ChatMessage::Started { user, .. } => {
                format!("{when} {user} started playback")
            }
            ChatMessage::Select { user, file, .. } => {
                format!("{when} {user} selected file: {file:?}")
            }
            ChatMessage::Chat { user, msg, .. } => {
                format!("{when} {user}: {msg}")
            }
            ChatMessage::Disconnected { .. } => {
                format!("{when} lost connection to server")
            }
            ChatMessage::Connected { .. } => {
                format!("{when} connection to server established")
            }
            ChatMessage::Seek { user, pos, .. } => {
                format!("{when} {user} seeked to {pos:?}")
            }
        };

        Container::new(Text::new(text))
            .style(style)
            .width(Length::Fill)
            .into()
    }

    pub fn style(&self, theme: Theme) -> iced::theme::Container {
        match self {
            ChatMessage::Disconnected { .. } => ContainerBackground::new(theme.palette().danger),
            _ => ContainerBackground::new(theme.palette().background),
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
