use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use iced::widget::{column, Button, TextInput};
use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::*;

use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::mpv::event::MpvEvent;
use crate::mpv::{Mpv, MpvResultingAction};
use crate::ws::{ServerMessage, ServerWebsocket, WebSocketMessage};

#[derive(Debug)]
pub enum MainWindow {
    Startup {
        url: String,
        user: String,
        path: String,
    },
    Running {
        db: Arc<FileDatabase>,
        ws: Arc<ServerWebsocket>,
        playlist_widget: PlaylistWidgetState,
        mpv: Mpv,
        filename: Option<String>,
        user: String,
        ready: bool,
    },
}

#[derive(Debug, Clone)]
pub enum MainMessage {
    WebSocket(WebSocketMessage),
    Mpv(MpvEvent),
    User(UserMessage),
    Database(DatabaseMessage),
    FileTable(PlaylistWidgetMessage),
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

    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        (
            Self::Startup {
                url: String::default(),
                user: String::default(),
                path: String::default(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match self.borrow_mut() {
            MainWindow::Startup { url, user, path } => match message {
                MainMessage::User(UserMessage::UsernameInput(u)) => *user = u,
                MainMessage::User(UserMessage::UrlInput(u)) => *url = u,
                MainMessage::User(UserMessage::PathInput(p)) => *path = p,
                MainMessage::User(UserMessage::StartButton) => {
                    let mpv = Mpv::new();
                    mpv.init().unwrap();
                    let db = Arc::new(FileDatabase::new(&[PathBuf::from_str(path).unwrap()]));
                    let cmd = FileDatabase::update_command(&db);
                    *self = MainWindow::Running {
                        playlist_widget: Default::default(),
                        mpv,
                        ws: Arc::new(ServerWebsocket::new(url.clone())),
                        // TODO do not unwrap
                        db,
                        ready: false,
                        user: user.clone(),
                        filename: None,
                    };
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
                filename,
            } => {
                match message {
                    MainMessage::FileTable(event) => match event {
                        PlaylistWidgetMessage::FileDoubleClick(f) => {
                            trace!("double: {f:?}");
                            let ws_cmd = ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Select {
                                    filename: f.name.clone(),
                                    username: user.clone(),
                                },
                            );
                            let find_cmd = FileDatabase::find_command(db, &f.name);
                            return Command::batch([ws_cmd, find_cmd]);
                        }
                        PlaylistWidgetMessage::FileMove(f, i) => {
                            trace!("move file: {f:?}, {i}");
                            let playlist = playlist_widget
                                .move_file(f, i)
                                .drain(..)
                                .map(|f| f.name)
                                .collect();
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Playlist {
                                    playlist,
                                    username: user.clone(),
                                },
                            );
                        }
                        PlaylistWidgetMessage::FileDelete(f) => {
                            trace!("delete file: {f:?}");
                            let playlist = playlist_widget
                                .delete_file(f)
                                .drain(..)
                                .map(|f| f.name)
                                .collect();
                            return ServerWebsocket::send_command(
                                ws,
                                ServerMessage::Playlist {
                                    playlist,
                                    username: user.clone(),
                                },
                            );
                        }
                        PlaylistWidgetMessage::FileInteraction(f, i) => {
                            trace!("file interaction: {f:?}, {i:?}");
                            playlist_widget.file_interaction(f, i)
                        }
                    },
                    MainMessage::Mpv(event) => match mpv.react_to(event) {
                        Ok(Some(MpvResultingAction::PlayNext)) => {}
                        Ok(Some(MpvResultingAction::Seek(position))) => {
                            if let Some(filename) = filename.clone() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Seek {
                                        filename,
                                        position,
                                        username: user.clone(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::ReOpenFile)) => {
                            if let Some(file) = filename {
                                return FileDatabase::find_command(db, file);
                            }
                        }
                        Ok(Some(MpvResultingAction::Pause)) => {
                            if let Some(filename) = filename.clone() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Pause {
                                        filename,
                                        username: user.clone(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::Start)) => {
                            if let Some(filename) = filename.clone() {
                                return ServerWebsocket::send_command(
                                    ws,
                                    ServerMessage::Start {
                                        filename,
                                        username: user.clone(),
                                    },
                                );
                            }
                        }
                        Ok(Some(MpvResultingAction::Exit)) => panic!("Mpv ended"),
                        Ok(None) => {}
                        Err(e) => error!("{e:?}"),
                    },
                    MainMessage::WebSocket(event) => match event {
                        WebSocketMessage::Received(msg) => {
                            //
                            match msg {
                                ServerMessage::Ping(uuid) => {
                                    return ServerWebsocket::send_command(
                                        ws,
                                        ServerMessage::Ping(uuid),
                                    )
                                }
                                ServerMessage::VideoStatus { filename, position } => {
                                    trace!("{filename}, {position:?}")
                                }
                                ServerMessage::StatusList(s) => debug!("{s:?}"),
                                ServerMessage::Pause {
                                    filename: _,
                                    username: _,
                                } => {
                                    //TODO do not unwrap
                                    mpv.pause(true).unwrap();
                                }
                                ServerMessage::Start {
                                    filename: _,
                                    username: _,
                                } => {
                                    //TODO do not unwrap
                                    mpv.pause(false).unwrap();
                                }
                                ServerMessage::Seek {
                                    filename: _,
                                    position,
                                    username: _,
                                } => {
                                    //TODO do not unwrap
                                    mpv.set_playback_position(position).unwrap();
                                }
                                ServerMessage::Select {
                                    filename,
                                    username: _,
                                } => {
                                    //TODO do not unwrap
                                    mpv.pause(true).unwrap();
                                    return FileDatabase::find_command(db, &filename);
                                }
                                ServerMessage::Message { message, username } => {
                                    trace!("{username}: {message}")
                                }
                                ServerMessage::Playlist {
                                    playlist,
                                    username: _,
                                } => playlist_widget.replace_files(playlist),
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
                        *ready ^= true;
                        return ServerWebsocket::send_command(
                            ws,
                            ServerMessage::Status {
                                ready: *ready,
                                username: user.clone(),
                            },
                        );
                    }
                    MainMessage::Database(DatabaseMessage::FindFinished(r)) => {
                        trace!("{r:?}");
                        //TODO do not unwrap
                        if let Ok(Some(file)) = r.as_ref() {
                            if let Ok(new_filename) = file.name.clone().into_string() {
                                if let Some(old_filename) = filename {
                                    if new_filename.eq(old_filename) {
                                        mpv.load_file(file.path.clone()).unwrap();
                                    }
                                }
                            }
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
            MainWindow::Startup { url, user, path } => column!(
                TextInput::new("Server Address", url)
                    .on_input(|u| MainMessage::User(UserMessage::UrlInput(u))),
                TextInput::new("Username", user)
                    .on_input(|u| MainMessage::User(UserMessage::UsernameInput(u))),
                TextInput::new("Filepath", path)
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
                filename: _,
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
            db: _,
            ready: _,
            user: _,
            filename: _,
        } = self
        {
            // TODO use .map() here instead
            let mpv = mpv.subscribe();
            let ws = ws.subscribe();

            return Subscription::batch([mpv, ws]);
        }
        Subscription::none()
    }
}
