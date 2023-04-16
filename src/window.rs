use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

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
use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState, Video};
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::mpv::event::MpvEvent;
use crate::mpv::{Mpv, MpvResultingAction};
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
        playing: Option<PlayingFile>,
        user: String,
        ready: bool,
        messages: Vec<String>,
        message: String,
        users: Vec<UserStatus>,
    },
}

#[derive(Debug, Clone)]
pub struct PlayingFile {
    video: Video,
    // TODO make an enum for wither url or pathbuf
    // TODO or clapse video and path
    path: Option<PathBuf>,
    heartbeat: bool,
    last_seek: Duration,
}

impl PlayingFile {
    pub fn subscribe(&self) -> Subscription<MainMessage> {
        if self.heartbeat {
            iced::subscription::channel(
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
            )
        } else {
            Subscription::none()
        }
    }
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
        (
            // TODO do not unwrap here
            Self::Startup { config },
            Command::none(),
        )
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
                        playing: None,
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
                playing,
                messages,
                message,
                users,
            } => {
                match msg {
                    MainMessage::FileTable(event) => match event {
                        PlaylistWidgetMessage::DoubleClick(f) => {
                            debug!("FileTable doubleclick: {f:?}");
                            let ws_cmd = ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Select {
                                    filename: f.as_str().to_string(),
                                    username: user.clone(),
                                },
                            );
                            *playing = Some(PlayingFile {
                                video: f,
                                path: None,
                                last_seek: Duration::ZERO,
                                heartbeat: false,
                            });
                            if let Some(playing) = playing.as_mut() {
                                match &playing.video {
                                    Video::File(filename) => {
                                        if let Ok(Some(file)) = db.find_file(filename) {
                                            playing.path = Some(file.path.clone());
                                            mpv.load_file(file.path).log();
                                        }
                                    }
                                    Video::Url(url) => {
                                        playing.path = Some(PathBuf::default());
                                        mpv.load_url(url.clone()).log();
                                    }
                                }
                            }
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
                            if let Some(prev_playing) = playing.as_mut() {
                                if let Some(next) = playlist_widget.next_video(&prev_playing.video)
                                {
                                    let ws_cmd = ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Select {
                                            filename: next.as_str().to_string(),
                                            username: user.clone(),
                                        },
                                    );
                                    *playing = Some(PlayingFile {
                                        video: next,
                                        path: None,
                                        heartbeat: false,
                                        last_seek: Duration::ZERO,
                                    });
                                    if let Some(playing) = playing.as_mut() {
                                        match &playing.video {
                                            Video::File(filename) => {
                                                if let Ok(Some(file)) = db.find_file(filename) {
                                                    playing.path = Some(file.path.clone());
                                                    mpv.load_file(file.path).log();
                                                }
                                            }
                                            Video::Url(url) => {
                                                playing.path = Some(PathBuf::default());
                                                mpv.load_url(url.clone()).log();
                                            }
                                        }
                                    }
                                    return ws_cmd;
                                }
                            }
                        }
                        Ok(Some(MpvResultingAction::Seek(position))) => {
                            debug!("Mpv process: seek {position:?}");
                            if let Some(playing) = playing.clone() {
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
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::ReOpenFile)) => {
                            debug!("Mpv process: re-open file");
                            if let Some(playing) = playing.as_mut() {
                                match &playing.video {
                                    Video::File(filename) => {
                                        if let Ok(Some(file)) = db.find_file(filename) {
                                            playing.path = Some(file.path.clone());
                                            mpv.load_file(file.path).log();
                                        } else {
                                            playing.path = None;
                                        }
                                    }
                                    Video::Url(url) => {
                                        playing.path = Some(PathBuf::default());
                                        mpv.load_url(url.clone()).log();
                                    }
                                }
                            }
                        }
                        Ok(Some(MpvResultingAction::Pause)) => {
                            debug!("Mpv process: pause");
                            if let Some(playing) = playing.clone() {
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
                            if let Some(playing) = playing.clone() {
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
                        Ok(Some(MpvResultingAction::StartHeartbeat)) => {
                            debug!("Mpv process: start heartbeat");
                            if let Some(playing) = playing.as_mut() {
                                //TODO Is this a race condition?
                                playing.heartbeat = true;
                                mpv.set_playback_position(playing.last_seek).log();
                            }
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
                                ServerMessage::VideoStatus { filename, position } => {
                                    trace!("{filename}, {position:?}")
                                }
                                ServerMessage::StatusList { users: usrs } => {
                                    debug!("{users:?}");
                                    *users = usrs;
                                }
                                ServerMessage::Pause {
                                    filename: _,
                                    username: _,
                                } => {
                                    debug!("Socket: received pause");
                                    mpv.pause(true).log();
                                }
                                ServerMessage::Start {
                                    filename: _,
                                    username: _,
                                } => {
                                    debug!("Socket: received start");
                                    mpv.pause(false).log();
                                }
                                ServerMessage::Seek {
                                    filename,
                                    position,
                                    username: _,
                                } => {
                                    debug!("Socket: received seek {position:?}");
                                    if filename.ne(playing
                                        .as_ref()
                                        .map(|p| p.video.as_str())
                                        .unwrap_or_default())
                                    {
                                        *playing = Some(PlayingFile {
                                            video: Video::from_string(filename),
                                            path: None,
                                            heartbeat: false,
                                            last_seek: position,
                                        });
                                        if let Some(playing) = playing.as_mut() {
                                            match &playing.video {
                                                Video::File(filename) => {
                                                    if let Ok(Some(file)) = db.find_file(filename) {
                                                        playing.path = Some(file.path.clone());
                                                        mpv.load_file(file.path).log();
                                                    }
                                                }
                                                Video::Url(url) => {
                                                    playing.path = Some(PathBuf::default());
                                                    mpv.load_url(url.clone()).log();
                                                }
                                            }
                                        }
                                    } else if let Some(last_playing) = playing.as_mut() {
                                        last_playing.last_seek = position;
                                        if last_playing.path.is_some() {
                                            mpv.set_playback_position(position).log();
                                        }
                                    }
                                }
                                ServerMessage::Select {
                                    filename,
                                    username: _,
                                } => {
                                    debug!("Socket: received select: {filename}");
                                    //TODO do not unwrap
                                    mpv.pause(true).unwrap();
                                    *playing = Some(PlayingFile {
                                        video: Video::from_string(filename),
                                        path: None,
                                        last_seek: Duration::ZERO,
                                        heartbeat: false,
                                    });
                                    if let Some(playing) = playing.as_mut() {
                                        match &playing.video {
                                            Video::File(filename) => {
                                                if let Ok(Some(file)) = db.find_file(filename) {
                                                    playing.path = Some(file.path.clone());
                                                    mpv.load_file(file.path).log();
                                                }
                                            }
                                            Video::Url(url) => {
                                                playing.path = Some(PathBuf::default());
                                                mpv.load_url(url.clone()).log();
                                            }
                                        }
                                    }
                                }
                                ServerMessage::Message { message, username } => {
                                    trace!("{username}: {message}");
                                    messages.push(format!("{username}: {message}"));
                                }
                                ServerMessage::Playlist {
                                    playlist,
                                    username: _,
                                } => playlist_widget.replace_videos(playlist),
                                ServerMessage::Status { ready, username } => {
                                    warn!("{username}: {ready:?}")
                                }
                            }
                        }
                        WebSocketMessage::TungError { err } => error!("{err:?}"),
                        WebSocketMessage::TungStringError { msg, err } => error!("{msg}, {err:?}"),
                        WebSocketMessage::SerdeError { msg, err } => error!("{msg}, {err:?}"),
                        WebSocketMessage::WsStreamEnded => error!("Websocket ended"),
                        WebSocketMessage::Connected => {
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
                                messages.push(msg.clone());
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
                            if let Some(playing) = playing.as_mut() {
                                if playing.path.is_none() {
                                    if let Ok(Some(file)) = db.find_file(playing.video.as_str()) {
                                        playing.path = Some(file.path.clone());
                                        mpv.load_file(file.path).log();
                                    }
                                }
                            }
                        }
                        DatabaseMessage::UpdateFinished(_) => debug!("Database: update finished"),
                    },
                    MainMessage::Heartbeat => {
                        debug!("Heartbeat");
                        if let Some(playing) = playing {
                            //TODO do not unwrap here
                            let position = mpv.get_playback_position().unwrap();
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::VideoStatus {
                                    filename: playing.video.as_str().to_string(),
                                    position,
                                },
                            );
                        }
                    }
                }
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        // container(column![].spacing(20).padding(20).max_width(600)).into()
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
                mpv: _,
                ws: _,
                db: _,
                ready,
                user: _,
                playing: _,
                messages,
                message,
                users,
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
                    .map(|u| Text::new(format!("{}: {:?}", u.username, u.ready)).into())
                    .collect::<Vec<_>>();

                row!(
                    column!(
                        Container::new(
                            Scrollable::new(Column::with_children(msgs)).id(Id::new("messages"))
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
                            Scrollable::new(Column::with_children(users)).id(Id::new("users"))
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
                    // .align_items(Alignment::End)
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
        if let MainWindow::Running {
            playlist_widget: _,
            mpv,
            ws,
            db,
            ready: _,
            user: _,
            playing,
            messages: _,
            message: _,
            users: _,
        } = self
        {
            // TODO use .map() here instead
            let mpv = mpv.subscribe();
            let ws = ws.subscribe();
            let heartbeat = playing
                .as_ref()
                .map(|p| p.subscribe())
                .unwrap_or(Subscription::none());
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
