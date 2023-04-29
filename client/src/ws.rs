use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Error, Result};
use arc_swap::ArcSwapOption;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::{FutureExt, SinkExt, StreamExt};
use iced::widget::{Row, Text};
use iced::{Command, Renderer, Subscription, Theme};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::user::ThisUser;
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
        paused: bool,
    },
    StatusList {
        rooms: HashMap<String, Vec<UserStatus>>,
    },
    Pause {
        filename: String,
        #[serde(skip_serializing)]
        username: String,
    },
    Start {
        filename: String,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
    pub username: String,
    pub ready: bool,
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

impl From<ServerMessage> for MainMessage {
    fn from(msg: ServerMessage) -> Self {
        WebSocketMessage::Received(msg).into()
    }
}

#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Received(ServerMessage),
    // TODO collapse errors into one variant
    Error {
        msg: Option<String>,
        err: Arc<Error>,
    },
    WsStreamEnded,
    Connected,
    SendFinished(Arc<Result<()>>),
}

impl From<WebSocketMessage> for MainMessage {
    fn from(msg: WebSocketMessage) -> Self {
        MainMessage::WebSocket(msg)
    }
}

type WsSink = SplitSink<WebSocketStream<TokioAdapter<TcpStream>>, TsMessage>;
type WsStream = SplitStream<WebSocketStream<TokioAdapter<TcpStream>>>;

#[derive(Debug)]
pub struct ServerWebsocket {
    sink: Arc<ArcSwapOption<Mutex<WsSink>>>,
    addr: String,
}

enum WsState {
    Disconnected {
        sink: Arc<ArcSwapOption<Mutex<WsSink>>>,
        addr: String,
    },
    Connected {
        sink: Arc<ArcSwapOption<Mutex<WsSink>>>,
        addr: String,
        stream: WsStream,
    },
}

impl ServerWebsocket {
    pub fn new(addr: String) -> Self {
        Self {
            sink: Default::default(),
            addr,
        }
    }

    pub fn send_command(ws: &Arc<Self>, msg: ServerMessage) -> Command<MainMessage> {
        async fn send(ws: Arc<ServerWebsocket>, msg: ServerMessage) -> MainMessage {
            MainMessage::WebSocket(WebSocketMessage::SendFinished(Arc::new(ws.send(msg).await)))
        }
        Command::single(iced_native::command::Action::Future(
            send(ws.clone(), msg).boxed(),
        ))
    }

    pub async fn send(&self, msg: ServerMessage) -> Result<()> {
        match self.sink.load_full() {
            None => bail!("No sink available"),
            Some(sink) => {
                sink.lock()
                    .await
                    .send(TsMessage::Text(serde_json::to_string(&msg)?))
                    .await?
            }
        }
        Ok(())
    }

    pub fn subscribe(&self) -> Subscription<MainMessage> {
        iced::subscription::unfold(
            std::any::TypeId::of::<Self>(),
            WsState::Disconnected {
                sink: self.sink.clone(),
                addr: self.addr.clone(),
            },
            |state| async move {
                match state {
                    WsState::Disconnected { sink, addr } => {
                        sink.store(None);
                        let ws = async_tungstenite::tokio::connect_async(&addr).await;
                        match ws {
                            Ok((ws, _)) => {
                                let (tx, stream) = ws.split();
                                sink.store(Some(Arc::new(Mutex::new(tx))));
                                (
                                    WebSocketMessage::Connected.into(),
                                    WsState::Connected { sink, addr, stream },
                                )
                            }
                            Err(err) => {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                (
                                    WebSocketMessage::Error {
                                        msg: None,
                                        err: Arc::new(err.into()),
                                    }
                                    .into(),
                                    WsState::Disconnected { sink, addr },
                                )
                            }
                        }
                    }
                    WsState::Connected {
                        sink,
                        addr,
                        mut stream,
                    } => match stream.next().await {
                        Some(msg) => match msg {
                            Ok(msg) => match msg.clone().into_text() {
                                Ok(msg) => match serde_json::from_str::<ServerMessage>(&msg) {
                                    Ok(server_msg) => (
                                        server_msg.into(),
                                        WsState::Connected { sink, addr, stream },
                                    ),
                                    Err(err) => (
                                        WebSocketMessage::Error {
                                            msg: msg.into(),
                                            err: Arc::new(err.into()),
                                        }
                                        .into(),
                                        WsState::Connected { sink, addr, stream },
                                    ),
                                },
                                Err(err) => (
                                    WebSocketMessage::Error {
                                        msg: format!("{msg:?}").into(),
                                        err: Arc::new(err.into()),
                                    }
                                    .into(),
                                    WsState::Connected { sink, addr, stream },
                                ),
                            },
                            Err(err) => (
                                WebSocketMessage::Error {
                                    msg: None,
                                    err: Arc::new(err.into()),
                                }
                                .into(),
                                WsState::Disconnected { sink, addr },
                            ),
                        },
                        None => (
                            WebSocketMessage::WsStreamEnded.into(),
                            WsState::Disconnected { sink, addr },
                        ),
                    },
                }
            },
        )
    }
}
