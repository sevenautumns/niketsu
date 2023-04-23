use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{column, row, Button, Column, Container, Scrollable, Text, TextInput};
use iced::{
    Alignment, Application, Command, Element, Length, Padding, Renderer, Subscription, Theme,
};
use log::*;

use crate::config::Config;
use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::messages::Messages;
use crate::mpv::event::MpvEvent;
use crate::mpv::{Mpv, MpvResultingAction};
use crate::styling::{ContainerBorder, ResultButton};
use crate::user::ThisUser;
use crate::video::Video;
use crate::ws::{ServerMessage, ServerWebsocket, UserStatus, WebSocketMessage};

#[derive(Debug)]
pub enum MainWindow {
    Startup {
        config: Config,
    },
    Running {
        db: Arc<FileDatabase>,
        ws: Arc<ServerWebsocket>,
        playlist_widget: PlaylistWidgetState,
        mpv: Mpv,
        user: ThisUser,
        messages: Messages,
        message: String,
        users: Vec<UserStatus>,
    },
}

#[derive(Debug, Clone)]
pub enum MainMessage {
    WebSocket(WebSocketMessage),
    Mpv(MpvEvent),
    User(UserMessage),
    Database(DatabaseMessage),
    FileTable(PlaylistWidgetMessage),
    Heartbeat,
}

#[derive(Debug, Clone)]
pub enum UserMessage {
    UsernameInput(String),
    UrlInput(String),
    PathInput(String),
    StartButton,
    ReadyButton,
    SendMessage,
    StopDbUpdate,
    StartDbUpdate,
    ScrollMessages(RelativeOffset),
    MessageInput(String),
}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = MainMessage;

    type Theme = Theme;

    type Flags = Config;

    fn new(config: Self::Flags) -> (Self, Command<Self::Message>) {
        (Self::Startup { config }, Command::none())
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn theme(&self) -> Self::Theme {
        Self::Theme::Dark
    }

    fn update(&mut self, msg: Self::Message) -> Command<Self::Message> {
        match self.borrow_mut() {
            MainWindow::Startup { config } => match msg {
                MainMessage::User(UserMessage::UsernameInput(u)) => config.username = u,
                MainMessage::User(UserMessage::UrlInput(u)) => config.url = u,
                MainMessage::User(UserMessage::PathInput(p)) => config.media_dir = p,
                MainMessage::User(UserMessage::StartButton) => {
                    config.save().log();
                    let mpv = Mpv::new();
                    mpv.init().unwrap();
                    let db = Arc::new(FileDatabase::new(&[
                        PathBuf::from_str(&config.media_dir).unwrap()
                    ]));
                    let cmd = FileDatabase::update_command(&db);
                    *self = MainWindow::Running {
                        playlist_widget: Default::default(),
                        mpv,
                        ws: Arc::new(ServerWebsocket::new(config.url.clone())),
                        db,
                        user: ThisUser::new(config.username.clone()),
                        messages: Default::default(),
                        message: Default::default(),
                        users: vec![],
                    };
                    info!("Changed Mode to Running");
                    return cmd;
                }
                _ => todo!(),
            },
            MainWindow::Running {
                playlist_widget,
                mpv,
                ws,
                db,
                user,
                messages,
                message,
                users,
            } => {
                match msg {
                    MainMessage::FileTable(event) => match event {
                        PlaylistWidgetMessage::DoubleClick(video) => {
                            debug!("FileTable doubleclick: {video:?}");
                            let ws_cmd = ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Select {
                                    filename: video.as_str().to_string().into(),
                                    username: user.name(),
                                },
                            );
                            mpv.load(video, None, true, db).log();
                            return ws_cmd;
                        }
                        PlaylistWidgetMessage::Move(f, i) => {
                            debug!("FileTable move file: {f:?}, {i}");
                            let playlist = playlist_widget
                                .move_video(f, i)
                                .drain(..)
                                .map(|v| v.as_str().to_string())
                                .collect();
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Playlist {
                                    playlist,
                                    username: user.name(),
                                },
                            );
                        }
                        PlaylistWidgetMessage::Delete(f) => {
                            debug!("FileTable delete file: {f:?}");
                            let playlist = playlist_widget
                                .delete_video(&f)
                                .drain(..)
                                .map(|v| v.as_str().to_string())
                                .collect();
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Playlist {
                                    playlist,
                                    username: user.name(),
                                },
                            );
                        }
                        PlaylistWidgetMessage::Interaction(f, i) => {
                            debug!("FileTable file interaction: {f:?}, {i:?}");
                            playlist_widget.file_interaction(f, i)
                        }
                    },
                    MainMessage::Mpv(event) => match mpv.react_to(event) {
                        Ok(Some(MpvResultingAction::PlayNext(video))) => {
                            debug!("Mpv process: play next");
                            if let Some(next) = playlist_widget.next_video(&video) {
                                let ws_cmd = ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Select {
                                        filename: next.as_str().to_string().into(),
                                        username: user.name(),
                                    },
                                );
                                mpv.load(next, None, true, db).log();
                                return ws_cmd;
                            } else {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Select {
                                        filename: None,
                                        username: user.name(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::Seek(position))) => {
                            debug!("Mpv process: seek {position:?}");
                            if let Some(playing) = mpv.playing() {
                                // TODO remove this env
                                if let Ok(bool) = std::env::var("DEBUG_NO_SEEK") {
                                    if bool.to_lowercase().eq("true") {
                                        return Command::none();
                                    }
                                }
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Seek {
                                        filename: playing.video.as_str().to_string(),
                                        position,
                                        username: user.name(),
                                        paused: mpv.paused(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::ReOpenFile)) => {
                            debug!("Mpv process: re-open file");
                            mpv.reload(db).log()
                        }
                        Ok(Some(MpvResultingAction::Pause)) => {
                            debug!("Mpv process: pause");
                            if let Some(playing) = mpv.playing() {
                                return Command::batch([
                                    user.set_ready(false, ws),
                                    ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Pause {
                                            filename: playing.video.as_str().to_string(),
                                            username: user.name(),
                                        },
                                    ),
                                ]);
                            }
                        }
                        Ok(Some(MpvResultingAction::Start)) => {
                            debug!("Mpv process: start");
                            if let Some(playing) = mpv.playing() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Start {
                                        filename: playing.video.as_str().to_string(),
                                        username: user.name(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::Exit)) => {
                            debug!("Mpv process: exit");
                            return iced::window::close();
                        }
                        Ok(None) => debug!("Mpv process: None"),
                        Err(e) => error!("Mpv error: {e:?}"),
                    },
                    MainMessage::WebSocket(event) => match event {
                        WebSocketMessage::Received(msg) => {
                            //
                            match msg {
                                ServerMessage::Ping { uuid } => {
                                    debug!("Socket: received ping {uuid}");
                                    return ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Ping { uuid },
                                    );
                                }
                                ServerMessage::VideoStatus {
                                    filename,
                                    position,
                                    paused,
                                } => {
                                    trace!("{filename:?}, {position:?}, {paused:?}")
                                }
                                ServerMessage::StatusList { users: usrs } => {
                                    debug!("{users:?}");
                                    *users = usrs;
                                }
                                ServerMessage::Pause { username, .. } => {
                                    debug!("Socket: received pause");
                                    mpv.pause(true).log();
                                    return messages.push_paused(username);
                                }
                                ServerMessage::Start { username, .. } => {
                                    debug!("Socket: received start");
                                    mpv.pause(false).log();
                                    return messages.push_started(username);
                                }
                                ServerMessage::Seek {
                                    filename,
                                    position,
                                    username,
                                    paused,
                                } => {
                                    debug!("Socket: received seek {position:?}");
                                    if !mpv.seeking() {
                                        mpv.seek(
                                            Video::from_string(filename.clone()),
                                            position,
                                            paused,
                                            db,
                                        )
                                        .log();
                                        return messages.push_seek(position, filename, username);
                                    }
                                }
                                ServerMessage::Select { filename, username } => {
                                    debug!("Socket: received select: {filename:?}");
                                    match filename.clone() {
                                        Some(filename) => mpv
                                            .load(Video::from_string(filename), None, true, db)
                                            .log(),
                                        None => mpv.unload(),
                                    }
                                    return messages.push_select(filename, username);
                                }
                                ServerMessage::Message { message, username } => {
                                    trace!("{username}: {message}");
                                    return messages.push_chat(message.clone(), username.clone());
                                }
                                ServerMessage::Playlist { playlist, username } => {
                                    playlist_widget.replace_videos(playlist);
                                    return messages.push_playlist_changed(username);
                                }
                                ServerMessage::Status { ready, username } => {
                                    warn!("{username}: {ready:?}")
                                }
                            }
                        }
                        WebSocketMessage::Error { msg, err } => error!("{msg:?}, {err:?}"),
                        WebSocketMessage::WsStreamEnded => {
                            error!("Websocket ended");
                            return messages.push_disconnected();
                        }
                        WebSocketMessage::Connected => {
                            trace!("Socket: connected");
                            return Command::batch([user.status(ws), messages.push_connected()]);
                        }
                        WebSocketMessage::SendFinished(r) => trace!("{r:?}"),
                    },
                    MainMessage::User(event) => match event {
                        UserMessage::ReadyButton => {
                            debug!("User: ready press");
                            return user.toggle_ready(ws);
                        }
                        UserMessage::SendMessage => {
                            if !message.is_empty() {
                                let msg = message.clone();
                                *message = Default::default();
                                return Command::batch([
                                    ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Message {
                                            message: msg.clone(),
                                            username: user.name(),
                                        },
                                    ),
                                    messages.push_chat(msg, user.name()),
                                ]);
                            }
                        }
                        UserMessage::MessageInput(msg) => *message = msg,
                        UserMessage::StartDbUpdate => {
                            trace!("Start database update request received");
                            return FileDatabase::update_command(db);
                        }
                        UserMessage::StopDbUpdate => {
                            trace!("Stop database update request received");
                            db.stop_update()
                        }
                        UserMessage::ScrollMessages(off) => messages.set_offset(off),
                        _ => {}
                    },
                    MainMessage::Database(event) => match event {
                        DatabaseMessage::Changed => {
                            trace!("Database: changed");
                            mpv.may_reload(db).log();
                        }
                        DatabaseMessage::UpdateFinished(_) => debug!("Database: update finished"),
                    },
                    MainMessage::Heartbeat => {
                        debug!("Heartbeat");
                        let playing = mpv.playing();
                        return ServerWebsocket::send_command(
                            ws,
                            ServerMessage::VideoStatus {
                                filename: playing.as_ref().map(|p| p.video.as_str().to_string()),
                                position: playing
                                    .map(|_| mpv.get_playback_position().ok())
                                    .flatten(),
                                paused: mpv.paused(),
                            },
                        );
                    }
                }
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        match self {
            MainWindow::Startup { config } => Container::new(
                Container::new(
                    column!(
                        TextInput::new("Server Address", &config.url)
                            .on_input(|u| MainMessage::User(UserMessage::UrlInput(u))),
                        TextInput::new("Username", &config.username)
                            .on_input(|u| MainMessage::User(UserMessage::UsernameInput(u))),
                        TextInput::new("Filepath", &config.media_dir)
                            .on_input(|p| MainMessage::User(UserMessage::PathInput(p))),
                        Button::new(
                            Text::new("Start")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .width(Length::Fill)
                        .on_press(MainMessage::User(UserMessage::StartButton))
                    )
                    .align_items(Alignment::Center)
                    .width(Length::Fill)
                    .spacing(10)
                    .padding(Padding::new(10.0)),
                )
                .height(Length::Shrink)
                .style(ContainerBorder::basic()),
            )
            .padding(Padding::new(5.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y()
            .into(),
            MainWindow::Running {
                playlist_widget,
                messages,
                message,
                users,
                user,
                mpv,
                db,
                ..
            } => {
                let mut btn;
                match user.ready() {
                    true => {
                        btn = Button::new(
                            Text::new("Ready")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .style(ResultButton::ready())
                    }
                    false => {
                        btn = Button::new(
                            Text::new("Not Ready")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .style(ResultButton::not_ready())
                    }
                }
                btn = btn.on_press(MainMessage::User(UserMessage::ReadyButton));

                let users = users
                    .iter()
                    .cloned()
                    .map(|u| u.to_text(user, &self.theme()).into())
                    .collect::<Vec<_>>();

                row!(
                    column!(
                        messages.view(self.theme()),
                        row!(
                            TextInput::new("Message", message)
                                .width(Length::Fill)
                                .on_input(|m| MainMessage::User(UserMessage::MessageInput(m)))
                                .on_submit(MainMessage::User(UserMessage::SendMessage)),
                            Button::new("Send")
                                .on_press(MainMessage::User(UserMessage::SendMessage))
                        )
                        .spacing(5.0)
                    )
                    .spacing(5.0)
                    .width(Length::Fill)
                    .height(Length::Fill),
                    column!(
                        db.view(),
                        Container::new(
                            Scrollable::new(Column::with_children(users))
                                .width(Length::Fill)
                                .id(Id::new("users"))
                        )
                        .style(ContainerBorder::basic())
                        .padding(5.0)
                        .width(Length::Fill)
                        .height(Length::Fill),
                        Container::new(PlaylistWidget::new(playlist_widget, mpv, db))
                            .style(ContainerBorder::basic())
                            .padding(5.0)
                            .height(Length::Fill),
                        btn.width(Length::Fill)
                    )
                    .width(Length::Fill)
                    .spacing(5.0)
                )
                .spacing(5.0)
                .padding(Padding::new(5.0))
                .into()
            }
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        if let MainWindow::Running { mpv, ws, db, .. } = self {
            // TODO use .map() here instead
            let heartbeat = iced::subscription::channel(
                std::any::TypeId::of::<Self>(),
                1,
                |mut output| async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if let Err(e) = output.try_send(MainMessage::Heartbeat) {
                            error!("{e:?}");
                        }
                    }
                },
            );
            let mpv = mpv.subscribe();
            let ws = ws.subscribe();
            let db = db.subscribe();

            return Subscription::batch([mpv, ws, db, heartbeat]);
        }
        Subscription::none()
    }
}

pub trait LogResult {
    fn log(&self);
}

impl<T> LogResult for anyhow::Result<T> {
    fn log(&self) {
        if let Err(e) = self {
            error!("{e:?}")
        }
    }
}
