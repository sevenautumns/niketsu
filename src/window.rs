use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use iced::theme::{Button as ButtonTheme, Container as ContainerTheme};
use iced::widget::button::{Appearance as ButtonAp, StyleSheet as ButtonSS};
use iced::widget::container::{Appearance as ContainerAp, StyleSheet as ContainerSS};
use iced::widget::scrollable::Id;
use iced::widget::{column, row, Button, Column, Container, Scrollable, Text, TextInput};
use iced::{
    Alignment, Application, Command, Element, Length, Padding, Renderer, Subscription, Theme,
};
use log::*;

use crate::config::Config;
use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::mpv::event::MpvEvent;
use crate::mpv::{Mpv, MpvResultingAction};
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
        user: String,
        ready: bool,
        messages: Vec<String>,
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
                        ready: false,
                        user: config.username.clone(),
                        messages: vec![],
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
                ready,
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
                                    filename: video.as_str().to_string(),
                                    username: user.clone(),
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
                                    username: user.clone(),
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
                                    username: user.clone(),
                                },
                            );
                        }
                        PlaylistWidgetMessage::Interaction(f, i) => {
                            debug!("FileTable file interaction: {f:?}, {i:?}");
                            playlist_widget.file_interaction(f, i)
                        }
                    },
                    MainMessage::Mpv(event) => match mpv.react_to(event) {
                        Ok(Some(MpvResultingAction::PlayNext)) => {
                            debug!("Mpv process: play next");
                            if let Some(prev_playing) = mpv.playing() {
                                if let Some(next) = playlist_widget.next_video(&prev_playing.video)
                                {
                                    let ws_cmd = ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Select {
                                            filename: next.as_str().to_string(),
                                            username: user.clone(),
                                        },
                                    );
                                    mpv.load(next, None, true, db).log();
                                    return ws_cmd;
                                }
                            }
                        }
                        Ok(Some(MpvResultingAction::Seek(position))) => {
                            debug!("Mpv process: seek {position:?}");
                            if let Some(playing) = mpv.playing() {
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
                                        username: user.clone(),
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
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Pause {
                                        filename: playing.video.as_str().to_string(),
                                        username: user.clone(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::Start)) => {
                            debug!("Mpv process: start");
                            if let Some(playing) = mpv.playing() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Start {
                                        filename: playing.video.as_str().to_string(),
                                        username: user.clone(),
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
                                    trace!("{filename}, {position:?}, {paused:?}")
                                }
                                ServerMessage::StatusList { users: usrs } => {
                                    debug!("{users:?}");
                                    *users = usrs;
                                }
                                ServerMessage::Pause { username, .. } => {
                                    messages.push(format!("{username} paused the video"));
                                    debug!("Socket: received pause");
                                    mpv.pause(true).log();
                                }
                                ServerMessage::Start { username, .. } => {
                                    messages.push(format!("{username} started the video"));
                                    debug!("Socket: received start");
                                    mpv.pause(false).log();
                                }
                                ServerMessage::Seek {
                                    filename,
                                    position,
                                    username,
                                    paused,
                                } => {
                                    debug!("Socket: received seek {position:?}");
                                    if !mpv.seeking() {
                                        messages.push(format!("{username} seeked to {position:?}"));
                                        mpv.seek(
                                            Video::from_string(filename),
                                            position,
                                            paused,
                                            db,
                                        )
                                        .log();
                                    }
                                }
                                ServerMessage::Select { filename, username } => {
                                    messages.push(format!("{username} changed file"));
                                    debug!("Socket: received select: {filename}");
                                    mpv.load(Video::from_string(filename), None, true, db).log();
                                }
                                ServerMessage::Message { message, username } => {
                                    trace!("{username}: {message}");
                                    messages.push(format!("{username}: {message}"));
                                }
                                ServerMessage::Playlist { playlist, username } => {
                                    playlist_widget.replace_videos(playlist);
                                    messages.push(format!("{username} changed playlist"));
                                }
                                ServerMessage::Status { ready, username } => {
                                    warn!("{username}: {ready:?}")
                                }
                            }
                        }
                        WebSocketMessage::TungError { err } => error!("{err:?}"),
                        WebSocketMessage::TungStringError { msg, err } => error!("{msg}, {err:?}"),
                        WebSocketMessage::SerdeError { msg, err } => error!("{msg}, {err:?}"),
                        WebSocketMessage::WsStreamEnded => {
                            messages.push(String::from("Server connection ended"));
                            error!("Websocket ended")
                        }
                        WebSocketMessage::Connected => {
                            messages.push(String::from("Connected to server"));
                            trace!("Socket: connected");
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Status {
                                    ready: *ready,
                                    username: user.clone(),
                                },
                            );
                        }
                        WebSocketMessage::SendFinished(r) => trace!("{r:?}"),
                    },
                    MainMessage::User(event) => match event {
                        UserMessage::ReadyButton => {
                            debug!("User: ready press");
                            *ready ^= true;
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Status {
                                    ready: *ready,
                                    username: user.clone(),
                                },
                            );
                        }
                        UserMessage::SendMessage => {
                            if !message.is_empty() {
                                let msg = message.clone();
                                *message = Default::default();
                                messages.push(format!("{user}: {msg}"));
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Message {
                                        message: msg,
                                        username: user.clone(),
                                    },
                                );
                            }
                        }
                        UserMessage::MessageInput(msg) => *message = msg,
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
                        if let Some(playing) = mpv.playing() {
                            if let Ok(position) = mpv.get_playback_position() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::VideoStatus {
                                        filename: playing.video.as_str().to_string(),
                                        position,
                                        paused: mpv.paused(),
                                    },
                                );
                            }
                        }
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
                ready,
                messages,
                message,
                users,
                user,
                ..
            } => {
                let mut btn;
                match ready {
                    true => {
                        btn = Button::new(
                            Text::new("Ready")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .style(ReadyTheme::ready())
                    }
                    false => {
                        btn = Button::new(
                            Text::new("Not Ready")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .style(ReadyTheme::not_ready())
                    }
                }
                btn = btn.on_press(MainMessage::User(UserMessage::ReadyButton));

                let msgs = messages
                    .iter()
                    .cloned()
                    .map(|m| Text::new(m).into())
                    .collect::<Vec<_>>();

                let users = users
                    .iter()
                    .cloned()
                    .map(|u| {
                        let mut username = u.username;
                        if username.eq(user) {
                            username = format!("(me) {username}");
                        }
                        Text::new(format!("{}: {:?}", username, u.ready)).into()
                    })
                    .collect::<Vec<_>>();

                row!(
                    column!(
                        Container::new(
                            Scrollable::new(Column::with_children(msgs))
                                .width(Length::Fill)
                                .id(Id::new("messages"))
                        )
                        .style(ContainerBorder::basic())
                        .padding(5.0)
                        .width(Length::Fill)
                        .height(Length::Fill),
                        row!(
                            TextInput::new("Message", message)
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
                        Container::new(
                            Scrollable::new(Column::with_children(users))
                                .width(Length::Fill)
                                .id(Id::new("users"))
                        )
                        .style(ContainerBorder::basic())
                        .padding(5.0)
                        .width(Length::Fill)
                        .height(Length::Fill),
                        Container::new(PlaylistWidget::new(playlist_widget))
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
            let heartbeat = mpv
                .playing()
                .map(|p| p.subscribe())
                .unwrap_or(Subscription::none());
            let mpv = mpv.subscribe();
            let ws = ws.subscribe();
            let db = db.subscribe();

            return Subscription::batch([mpv, ws, db, heartbeat]);
        }
        Subscription::none()
    }
}

pub struct ReadyTheme {
    ready: bool,
}

impl ReadyTheme {
    pub fn not_ready() -> iced::theme::Button {
        ButtonTheme::Custom(Box::new(Self { ready: false }))
    }

    pub fn ready() -> iced::theme::Button {
        ButtonTheme::Custom(Box::new(Self { ready: true }))
    }

    pub fn background(&self, style: &Theme) -> Option<iced::Background> {
        match self.ready {
            true => Some(iced::Background::Color(style.palette().success)),
            false => Some(iced::Background::Color(style.palette().danger)),
        }
    }
}

impl ButtonSS for ReadyTheme {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> ButtonAp {
        ButtonAp {
            background: self.background(style),
            ..style.active(&iced::theme::Button::Text)
        }
    }

    fn hovered(&self, style: &Self::Style) -> ButtonAp {
        ButtonAp {
            background: self.background(style),
            ..style.hovered(&iced::theme::Button::Text)
        }
    }

    fn pressed(&self, style: &Self::Style) -> ButtonAp {
        ButtonAp {
            background: self.background(style),
            ..style.pressed(&iced::theme::Button::Text)
        }
    }

    fn disabled(&self, style: &Self::Style) -> ButtonAp {
        ButtonAp {
            background: self.background(style),
            ..style.disabled(&iced::theme::Button::Text)
        }
    }
}

pub struct ContainerBorder;

impl ContainerBorder {
    pub fn basic() -> iced::theme::Container {
        ContainerTheme::Custom(Box::new(Self))
    }
}

impl ContainerSS for ContainerBorder {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> ContainerAp {
        ContainerAp {
            border_color: style.palette().text,
            border_radius: 5.0,
            border_width: 2.0,
            ..Default::default()
        }
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
