use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use chrono::{DateTime, Local};
use crossbeam_channel::{Receiver as MsgsReceiver, Sender as MsgsSender};
use elsa::vec::FrozenVec;
use iced::widget::scrollable::Id;
use iced::widget::{Column, Container, Scrollable, Text};
use iced::{Element, Length, Renderer, Theme};

use crate::client::LogResult;
use crate::styling::{ContainerBackground, ContainerBorder};
use crate::window::MainMessage;

pub struct MessagesReceiver {
    messages: FrozenVec<Box<ChatMessage>>,
    recv: MsgsReceiver<ChatMessage>,
}

impl std::fmt::Debug for MessagesReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO debug messages
        f.debug_struct("MessageReceiver")
            .field("recv", &self.recv)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct MessagesSender {
    send: Arc<MsgsSender<ChatMessage>>,
}

pub fn messages_pair() -> (MessagesSender, MessagesReceiver) {
    let (send, recv) = crossbeam_channel::unbounded::<ChatMessage>();
    (MessagesSender::new(send), MessagesReceiver::new(recv))
}

impl MessagesReceiver {
    pub fn new(recv: MsgsReceiver<ChatMessage>) -> Self {
        Self {
            messages: FrozenVec::new(),
            recv,
        }
    }

    pub fn view<'a>(&self, theme: Theme) -> Element<'a, MainMessage, Renderer> {
        while let Ok(msg) = self.recv.try_recv() {
            self.messages.push(Box::new(msg));
        }

        let msgs = self
            .messages
            .iter()
            .map(|m| m.to_text(theme.clone()))
            .collect();
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

    pub fn push_user_chat(&self, msg: String, user: String) {
        self.messages.push(Box::new(ChatMessage::UserChat {
            when: Local::now(),
            user,
            msg,
        }));
    }
}

impl MessagesSender {
    pub fn new(send: MsgsSender<ChatMessage>) -> Self {
        Self {
            send: Arc::new(send),
        }
    }

    pub fn push_playlist_changed(&self, user: String) {
        self.send
            .send(ChatMessage::PlaylistChanged {
                when: Local::now(),
                user,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_paused(&self, user: String) {
        self.send
            .send(ChatMessage::Paused {
                when: Local::now(),
                user,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_playback_speed(&self, speed: f64, user: String) {
        self.send
            .send(ChatMessage::PlaybackSpeed {
                when: Local::now(),
                user,
                speed,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_started(&self, user: String) {
        self.send
            .send(ChatMessage::Started {
                when: Local::now(),
                user,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_select(&self, file: Option<String>, user: String) {
        self.send
            .send(ChatMessage::Select {
                when: Local::now(),
                user,
                file,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_seek(&self, pos: Duration, file: String, desync: bool, user: String) {
        self.send
            .send(ChatMessage::Seek {
                when: Local::now(),
                user,
                file,
                pos,
                desync,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_user_chat(&self, msg: String, user: String) {
        self.send
            .send(ChatMessage::UserChat {
                when: Local::now(),
                user,
                msg,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_server_chat(&self, msg: String, error: bool) {
        self.send
            .send(ChatMessage::ServerChat {
                when: Local::now(),
                error,
                msg,
            })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_connected(&self) {
        self.send
            .send(ChatMessage::Connected { when: Local::now() })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_disconnected(&self) {
        self.send
            .send(ChatMessage::Disconnected { when: Local::now() })
            .map_err(Error::msg)
            .log();
    }

    pub fn push_connection_error(&self, error: String) {
        self.send
            .send(ChatMessage::ConnectionError {
                when: Local::now(),
                error,
            })
            .map_err(Error::msg)
            .log();
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
    ConnectionError {
        when: DateTime<Local>,
        error: String,
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
            ChatMessage::PlaybackSpeed { user, speed, .. } => {
                format!("{when} {user} changed playback speed to {speed:.5}")
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
            ChatMessage::ConnectionError { error, .. } => {
                format!("{when} Connection Error: {error}")
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
            ChatMessage::ConnectionError { when, .. } => when,
        }
    }
}
