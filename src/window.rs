use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{column, Button, TextInput};
use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::*;

use crate::config::Config;
use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState, Video};
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::mpv::event::MpvEvent;
use crate::mpv::{Mpv, MpvResultingAction};
use crate::ws::{ServerMessage, ServerWebsocket, WebSocketMessage};

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

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match self.borrow_mut() {
            MainWindow::Startup { config } => match message {
                MainMessage::User(UserMessage::UsernameInput(u)) => config.username = u,
                MainMessage::User(UserMessage::UrlInput(u)) => config.url = u,
                MainMessage::User(UserMessage::PathInput(p)) => config.media_dir = p,
                MainMessage::User(UserMessage::StartButton) => {
                    // TODO do not unwrap here
                    config.save().unwrap();
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
            } => {
                match message {
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
                                        if let Ok(Some(file)) = db.find_file(&filename) {
                                            playing.path = Some(file.path.clone());
                                            // TODO do not unwrap
                                            mpv.load_file(file.path).unwrap();
                                        }
                                    }
                                    Video::Url(url) => {
                                        playing.path = Some(PathBuf::default());
                                        // TODO do not unwrap
                                        mpv.load_url(url.clone()).unwrap();
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
                            if let Some(prev_playing) = playing.as_mut() {
                                if let Some(next) = playlist_widget.next_video(&prev_playing.video)
                                {
                                    *playing = Some(PlayingFile {
                                        video: next,
                                        path: None,
                                        heartbeat: false,
                                        last_seek: Duration::ZERO,
                                    });
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
                                        if let Ok(Some(file)) = db.find_file(&filename) {
                                            playing.path = Some(file.path.clone());
                                            // TODO do not unwrap
                                            mpv.load_file(file.path).unwrap();
                                        } else {
                                            playing.path = None;
                                        }
                                    }
                                    Video::Url(url) => {
                                        playing.path = Some(PathBuf::default());
                                        // TODO do not unwrap
                                        mpv.load_url(url.clone()).unwrap();
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
                                // TODO do not unwrap here
                                mpv.set_playback_position(playing.last_seek).unwrap();
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
                                ServerMessage::StatusList { users } => debug!("{users:?}"),
                                ServerMessage::Pause {
                                    filename: _,
                                    username: _,
                                } => {
                                    debug!("Socket: received pause");
                                    //TODO do not unwrap
                                    mpv.pause(true).unwrap();
                                }
                                ServerMessage::Start {
                                    filename: _,
                                    username: _,
                                } => {
                                    debug!("Socket: received start");
                                    //TODO do not unwrap
                                    mpv.pause(false).unwrap();
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
                                                    if let Ok(Some(file)) = db.find_file(&filename)
                                                    {
                                                        playing.path = Some(file.path.clone());
                                                        //TODO do not unwrap
                                                        mpv.load_file(file.path).unwrap();
                                                    }
                                                }
                                                Video::Url(url) => {
                                                    playing.path = Some(PathBuf::default());
                                                    // TODO do not unwrap
                                                    mpv.load_url(url.clone()).unwrap();
                                                }
                                            }
                                        }
                                    } else if let Some(last_playing) = playing.as_mut() {
                                        last_playing.last_seek = position;
                                        //TODO do not unwrap
                                        mpv.set_playback_position(position).unwrap();
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
                                                if let Ok(Some(file)) = db.find_file(&filename) {
                                                    playing.path = Some(file.path.clone());
                                                    //TODO do not unwrap
                                                    mpv.load_file(file.path).unwrap();
                                                }
                                            }
                                            Video::Url(url) => {
                                                playing.path = Some(PathBuf::default());
                                                // TODO do not unwrap
                                                mpv.load_url(url.clone()).unwrap();
                                            }
                                        }
                                    }
                                }
                                ServerMessage::Message { message, username } => {
                                    trace!("{username}: {message}")
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
                    MainMessage::User(UserMessage::ReadyButton) => {
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
                    MainMessage::Database(event) => match event {
                        DatabaseMessage::Changed => {
                            trace!("Database: changed");
                            if let Some(playing) = playing.as_mut() {
                                if playing.path.is_none() {
                                    if let Ok(Some(file)) = db.find_file(playing.video.as_str()) {
                                        playing.path = Some(file.path.clone());
                                        //TODO do not unwrap here
                                        mpv.load_file(file.path).unwrap();
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
                    _ => {}
                }
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        // container(column![].spacing(20).padding(20).max_width(600)).into()
        match self {
            MainWindow::Startup { config } => column!(
                TextInput::new("Server Address", &config.url)
                    .on_input(|u| MainMessage::User(UserMessage::UrlInput(u))),
                TextInput::new("Username", &config.username)
                    .on_input(|u| MainMessage::User(UserMessage::UsernameInput(u))),
                TextInput::new("Filepath", &config.media_dir)
                    .on_input(|p| MainMessage::User(UserMessage::PathInput(p))),
                Button::new("Start").on_press(MainMessage::User(UserMessage::StartButton))
            )
            .into(),
            MainWindow::Running {
                playlist_widget,
                mpv: _,
                ws: _,
                db: _,
                ready: _,
                user: _,
                playing: _,
            } => column!(
                PlaylistWidget::new(playlist_widget),
                Button::new("Ready").on_press(MainMessage::User(UserMessage::ReadyButton))
            )
            .into(),
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
