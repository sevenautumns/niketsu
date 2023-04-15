use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use arc_swap::ArcSwapOption;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::{Error as TsError, Message as TsMessage};
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::{FutureExt, SinkExt, StreamExt};
use iced::{Command, Subscription};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::window::MainMessage;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum ServerMessage {
    Ping(#[serde(rename = "uuid")] String),
    VideoStatus {
        filename: String,
        #[serde(with = "serde_millis")]
        position: Duration,
    },
    StatusList(#[serde(rename = "users")] Vec<UserStatus>),
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
    },
    Select {
        filename: String,
        #[serde(skip_serializing)]
        username: String,
    },
    Message {
        message: String,
        #[serde(skip_serializing)]
        username: String,
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

impl From<ServerMessage> for MainMessage {
    fn from(msg: ServerMessage) -> Self {
        WebSocketMessage::Received(msg).into()
    }
}

#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Received(ServerMessage),
    TungError {
        err: Arc<TsError>,
    },
    TungStringError {
        msg: TsMessage,
        err: Arc<TsError>,
    },
    SerdeError {
        msg: String,
        err: Arc<serde_json::Error>,
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
                                // TODO is 1 second sensible?
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                (
                                    WebSocketMessage::TungError { err: Arc::new(err) }.into(),
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
                                        WebSocketMessage::SerdeError {
                                            msg,
                                            err: Arc::new(err),
                                        }
                                        .into(),
                                        WsState::Connected { sink, addr, stream },
                                    ),
                                },
                                Err(err) => (
                                    WebSocketMessage::TungStringError {
                                        msg,
                                        err: Arc::new(err),
                                    }
                                    .into(),
                                    WsState::Connected { sink, addr, stream },
                                ),
                            },
                            Err(err) => (
                                WebSocketMessage::TungError { err: Arc::new(err) }.into(),
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
