use std::collections::{BTreeMap, BTreeSet};
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::SplitStream;
use futures::{SinkExt, StreamExt};
use iced::widget::{Row, Text};
use iced::{Renderer, Theme};
use log::{debug, error, trace, warn};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use tokio_native_tls::TlsStream;

use crate::client::{ClientInner, LogResult, PlayerMessage};
use crate::file_table::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::user::ThisUser;
use crate::video::Video;
use crate::window::MainMessage;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum ServerMessage {
    Ping {
        uuid: String,
    },
    Join {
        password: String,
        room: String,
        username: String,
    },
    VideoStatus {
        filename: Option<String>,
        #[serde(with = "serde_millis")]
        position: Option<Duration>,
        speed: f64,
        paused: bool,
    },
    StatusList {
        rooms: BTreeMap<String, BTreeSet<UserStatus>>,
    },
    Pause {
        #[serde(skip_serializing)]
        username: String,
    },
    Start {
        #[serde(skip_serializing)]
        username: String,
    },
    PlaybackSpeed {
        speed: f64,
        #[serde(skip_serializing)]
        username: String,
    },
    Seek {
        filename: String,
        #[serde(with = "serde_millis")]
        position: Duration,
        #[serde(skip_serializing)]
        username: String,
        paused: bool,
        speed: f64,
        #[serde(skip_serializing)]
        desync: bool,
    },
    Select {
        filename: Option<String>,
        #[serde(skip_serializing)]
        username: String,
    },
    UserMessage {
        message: String,
        #[serde(skip_serializing)]
        username: String,
    },
    ServerMessage {
        message: String,
        error: bool,
    },
    Playlist {
        playlist: Vec<String>,
        #[serde(skip_serializing)]
        username: String,
    },
    Status {
        ready: bool,
        username: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub username: String,
    pub ready: bool,
}

impl PartialEq for UserStatus {
    fn eq(&self, other: &Self) -> bool {
        self.username.eq(&other.username)
    }
}

impl Ord for UserStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.username.cmp(&other.username)
    }
}
impl PartialOrd for UserStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.username.partial_cmp(&other.username)
    }
}

impl UserStatus {
    pub fn to_text<'a>(&self, user: &ThisUser, theme: &Theme) -> Row<'a, MainMessage, Renderer> {
        let mut row = Row::new();
        if self.username.eq(&user.name()) {
            row = row.push(Text::new("(me) "));
        }
        let ready = match self.ready {
            true => Text::new("Ready").style(theme.palette().success),
            false => Text::new("Not Ready").style(theme.palette().danger),
        };
        row.push(Text::new(format!("{}: ", self.username)))
            .push(ready)
    }
}

#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Received(ServerMessage),
    Error(Arc<Error>),
    WsStreamEnded,
    Connected,
    SendFinished(Arc<Result<()>>),
}

type WsStream = SplitStream<
    WebSocketStream<
        Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<tokio::net::TcpStream>>>,
    >,
>;

#[derive(Debug, Clone)]
pub struct ServerWebsocket {
    addr: String,
    client_sender: MpscSender<PlayerMessage>,
    msg_sender: MpscSender<ServerMessage>,
}

impl ServerWebsocket {
    pub fn reboot(&self) -> Self {
        // TODO only reboot if msg_sender is closed?
        Self::new(self.addr.clone(), self.client_sender.clone())
    }

    pub fn new(addr: String, client_sender: MpscSender<PlayerMessage>) -> Self {
        let (msg_sender, mut rx): (MpscSender<ServerMessage>, _) =
            tokio::sync::mpsc::unbounded_channel();
        let addr2 = addr.clone();
        let client_sender2 = client_sender.clone();
        tokio::spawn(async move {
            let addr = addr2;
            let client_sender = client_sender2;
            let client_sender2 = client_sender.clone();
            let connection = async_tungstenite::tokio::connect_async(addr).await;
            match connection {
                Ok((ws, _)) => {
                    client_sender
                        .send(PlayerMessage::Server(WebSocketMessage::Connected))
                        .expect("Client sender unexpectedly ended");
                    let (mut sink, mut stream) = ws.split();
                    tokio::spawn(async move {
                        let client_sender = client_sender2;
                        loop {
                            if let Some(msg) = rx.recv().await {
                                match serde_json::to_string(&msg) {
                                    Ok(msg) => {
                                        if let Err(err) = sink.send(TsMessage::Text(msg)).await {
                                            client_sender
                                                .send(PlayerMessage::Server(
                                                    WebSocketMessage::Error(Arc::new(err.into())),
                                                ))
                                                .expect("Client sender unexpectedly ended");
                                        }
                                    }
                                    Err(err) => client_sender
                                        .send(PlayerMessage::Server(WebSocketMessage::Error(
                                            Arc::new(err.into()),
                                        )))
                                        .expect("Client sender unexpectedly ended"),
                                }
                            } else {
                                error!("Server message sender unexpectedly ended");
                                exit(1);
                            }
                        }
                    });
                    loop {
                        match Self::recv(&mut stream).await {
                            Ok(msg) => client_sender
                                .send(PlayerMessage::Server(msg))
                                .expect("Client sender unexpectedly ended"),
                            Err(err) => {
                                error!("Websocket ended: {err:?}");
                                client_sender
                                    .send(PlayerMessage::Server(WebSocketMessage::WsStreamEnded))
                                    .expect("Client sender unexpectedly ended")
                            }
                        }
                    }
                }
                Err(_) => {
                    client_sender.send(PlayerMessage::Server(WebSocketMessage::WsStreamEnded))
                }
            }
        });
        Self {
            addr,
            client_sender,
            msg_sender,
        }
    }

    pub fn send(&self, msg: ServerMessage) -> Result<()> {
        self.msg_sender.send(msg)?;
        Ok(())
    }

    async fn recv(stream: &mut WsStream) -> Result<WebSocketMessage> {
        if let Some(msg) = stream.next().await {
            match msg {
                Ok(msg) => {
                    let msg = msg.into_text()?;
                    let msg = serde_json::from_str::<ServerMessage>(&msg)?;
                    return Ok(WebSocketMessage::Received(msg));
                }
                Err(err) => {
                    error!("{err}");
                    return Ok(WebSocketMessage::WsStreamEnded);
                }
            }
        }
        Ok(WebSocketMessage::WsStreamEnded)
    }
}

impl ClientInner {
    pub fn react_to_server(&mut self, event: WebSocketMessage) -> Result<()> {
        match event {
            WebSocketMessage::Received(msg) => {
                //
                match msg {
                    ServerMessage::Ping { uuid } => {
                        debug!("Socket: received ping {uuid}");
                        self.ws.load().send(ServerMessage::Ping { uuid })?;
                        Ok(())
                    }
                    ServerMessage::VideoStatus {
                        filename,
                        position,
                        paused,
                        speed,
                    } => {
                        trace!("{filename:?}, {position:?}, {paused:?}, {speed:?}");
                        Ok(())
                    }
                    ServerMessage::StatusList { rooms } => {
                        debug!("Socket: received rooms: {rooms:?}");
                        self.rooms_widget.rcu(|r| {
                            let mut rs = RoomsWidgetState::clone(r);
                            rs.replace_rooms(rooms.clone());
                            rs
                        });
                        Ok(())
                    }
                    ServerMessage::Pause { username, .. } => {
                        debug!("Socket: received pause");
                        self.mpv.pause(true)?;
                        self.messages.push_paused(username);
                        Ok(())
                    }
                    ServerMessage::Start { username, .. } => {
                        debug!("Socket: received start");
                        self.mpv.pause(false)?;
                        self.messages.push_started(username);
                        Ok(())
                    }
                    ServerMessage::Seek {
                        filename,
                        position,
                        username,
                        paused,
                        desync,
                        speed,
                    } => {
                        debug!("Socket: received seek {position:?}");
                        if !self.mpv.seeking() {
                            self.mpv
                                .seek(
                                    Video::from_string(filename.clone()),
                                    position,
                                    paused,
                                    speed,
                                    &self.db,
                                )
                                .log();
                            self.messages
                                .push_seek(position, filename, desync, username);
                        }
                        Ok(())
                    }
                    ServerMessage::Select { filename, username } => {
                        debug!("Socket: received select: {filename:?}");
                        match filename.clone() {
                            Some(filename) => self
                                .mpv
                                .load(Video::from_string(filename), None, true, &self.db)
                                .log(),
                            None => self.mpv.unload(),
                        }
                        self.messages.push_select(filename, username);
                        Ok(())
                    }
                    ServerMessage::UserMessage { message, username } => {
                        trace!("Socket: received: {username}: {message}");
                        self.messages.push_user_chat(message, username);
                        Ok(())
                    }
                    ServerMessage::Playlist { playlist, username } => {
                        trace!("Socket: received playlist: {username}");
                        self.playlist_widget.rcu(|p| {
                            let mut plist = PlaylistWidgetState::clone(p);
                            plist.replace_videos(playlist.clone());
                            plist
                        });
                        self.messages.push_playlist_changed(username);
                        Ok(())
                    }
                    ServerMessage::Status { ready, username } => {
                        warn!("Received: {username}: {ready:?}");
                        Ok(())
                    }
                    ServerMessage::Join { room, username, .. } => {
                        warn!("Received: {room}: {username}");
                        Ok(())
                    }
                    ServerMessage::ServerMessage { message, error } => {
                        trace!("Socket: received server message: {error}: {message}");
                        self.messages.push_server_chat(message, error);
                        Ok(())
                    }
                    ServerMessage::PlaybackSpeed { speed, username } => {
                        trace!("Socket: received playback speed: {username}, {speed}");
                        self.mpv.set_playback_speed(speed).log();
                        self.messages.push_playback_speed(speed, username);
                        Ok(())
                    }
                }
            }
            WebSocketMessage::Error(err) => {
                warn!("Connection Error: {err}");
                self.messages.push_connection_error(err.to_string());
                Ok(())
            }
            WebSocketMessage::WsStreamEnded => {
                error!("Websocket ended");
                self.messages.push_disconnected();
                let ws = self.ws.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    ws.rcu(|w| {
                        let ws = ServerWebsocket::clone(w);
                        ws.reboot();
                        ws
                    })
                });
                Ok(())
            }
            WebSocketMessage::Connected => {
                trace!("Socket: connected");
                self.ws.load().send(ServerMessage::Join {
                    password: self.config.password.clone(),
                    room: self.config.room.clone(),
                    username: self.config.username.clone(),
                })?;
                self.ws.load().send(self.user.load().status())?;
                self.messages.push_connected();
                Ok(())
            }
            WebSocketMessage::SendFinished(r) => {
                trace!("{r:?}");

                Ok(())
            }
        }
    }
}
