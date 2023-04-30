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

    pub fn push_playback_speed(&mut self, speed: f64, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::PlaybackSpeed {
            when: Local::now(),
            user,
            speed,
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

    pub fn push_seek(
        &mut self,
        pos: Duration,
        file: String,
        desync: bool,
        user: String,
    ) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::Seek {
            when: Local::now(),
            user,
            file,
            pos,
            desync,
        });
        self.snap_scroll()
    }

    pub fn push_user_chat(&mut self, msg: String, user: String) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::UserChat {
            when: Local::now(),
            user,
            msg,
        });
        self.snap_scroll()
    }

    pub fn push_server_chat(&mut self, msg: String, error: bool) -> Command<MainMessage> {
        self.msgs.push(ChatMessage::ServerChat {
            when: Local::now(),
            error,
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
    PlaybackSpeed {
        when: DateTime<Local>,
        user: String,
        speed: f64,
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
        desync: bool,
    },
    UserChat {
        when: DateTime<Local>,
        user: String,
        msg: String,
    },
    ServerChat {
        when: DateTime<Local>,
        error: bool,
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
            ChatMessage::PlaybackSpeed { when, user, speed } => {
                format!("{when} {user} changed playback speed to {speed}")
            }
            ChatMessage::Select { user, file, .. } => {
                format!("{when} {user} selected file: {file:?}")
            }
            ChatMessage::UserChat { user, msg, .. } => {
                format!("{when} {user}: {msg}")
            }
            ChatMessage::Disconnected { .. } => {
                format!("{when} lost connection to server")
            }
            ChatMessage::Connected { .. } => {
                format!("{when} connection to server established")
            }
            ChatMessage::Seek {
                user, pos, desync, ..
            } => {
                let desync = desync.then(|| " due to desync").unwrap_or_default();
                format!("{when} {user} seeked to {pos:?}{desync}")
            }
            ChatMessage::ServerChat { msg, .. } => {
                format!("{when} {msg}")
            }
        };

        Container::new(Text::new(text))
            .style(style)
            .width(Length::Fill)
            .into()
    }

    pub fn style(&self, theme: Theme) -> iced::theme::Container {
        match self {
            ChatMessage::Disconnected { .. } => ContainerBackground::theme(theme.palette().danger),
            ChatMessage::ServerChat { error: true, .. } => {
                ContainerBackground::theme(theme.palette().danger)
            }
            _ => ContainerBackground::theme(theme.palette().background),
        }
    }

    pub fn when(&self) -> &DateTime<Local> {
        match self {
            ChatMessage::PlaylistChanged { when, .. } => when,
            ChatMessage::Paused { when, .. } => when,
            ChatMessage::Started { when, .. } => when,
            ChatMessage::Select { when, .. } => when,
            ChatMessage::UserChat { when, .. } => when,
            ChatMessage::Disconnected { when } => when,
            ChatMessage::Connected { when } => when,
            ChatMessage::Seek { when, .. } => when,
            ChatMessage::ServerChat { when, .. } => when,
            ChatMessage::PlaybackSpeed { when, .. } => when,
        }
    }
}
