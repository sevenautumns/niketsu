use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::SplitStream;
use futures::{SinkExt, StreamExt};
use iced::widget::{Row, Text};
use iced::{Renderer, Theme};
use log::{error, warn};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use tokio_native_tls::TlsStream;
use url::Url;

use self::message::{Connected, Received, WebSocketMessage, WsStreamEnded};
use crate::client::server::message::ServerError;
use crate::client::PlayerMessage;
use crate::iced_window::MainMessage;
use crate::user::ThisUser;

pub mod message;

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

type WsStream = SplitStream<
    WebSocketStream<
        Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<tokio::net::TcpStream>>>,
    >,
>;

#[derive(Debug, Clone)]
pub struct ServerWebsocket {
    addr: Url,
    client_sender: MpscSender<PlayerMessage>,
    msg_sender: MpscSender<ServerMessage>,
}

impl ServerWebsocket {
    #[must_use]
    pub fn reboot(self) -> Self {
        // TODO only reboot if msg_sender is closed?
        Self::new(self.addr.clone(), self.client_sender)
    }

    pub fn new(addr: Url, client_sender: MpscSender<PlayerMessage>) -> Self {
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
                        .send(WebSocketMessage::from(Connected).into())
                        .expect("Client sender unexpectedly ended");
                    let (mut sink, mut stream) = ws.split();
                    tokio::spawn(async move {
                        let client_sender = client_sender2;
                        loop {
                            if let Some(msg) = rx.recv().await {
                                match serde_json::to_string(&msg) {
                                    Ok(msg) => {
                                        if let Err(err) = sink.send(TsMessage::Text(msg)).await {
                                            warn!("Websocket ended: {err:?}");
                                            client_sender
                                                .send(
                                                    WebSocketMessage::from(ServerError(Arc::new(
                                                        err.into(),
                                                    )))
                                                    .into(),
                                                )
                                                .expect("Client sender unexpectedly ended");
                                            return;
                                        }
                                    }
                                    Err(err) => client_sender
                                        .send(
                                            WebSocketMessage::from(ServerError(Arc::new(
                                                err.into(),
                                            )))
                                            .into(),
                                        )
                                        .expect("Client sender unexpectedly ended"),
                                }
                            } else {
                                // error!("Server message sender unexpectedly ended");
                                // exit(1);
                                return;
                            }
                        }
                    });
                    loop {
                        match Self::recv(&mut stream).await {
                            Ok(msg @ WebSocketMessage::WsStreamEnded(_)) => {
                                client_sender
                                    .send(msg.into())
                                    .expect("Client sender unexpectedly ended");
                                return;
                            }
                            Ok(msg) => {
                                client_sender
                                    .send(msg.into())
                                    .expect("Client sender unexpectedly ended");
                            }
                            Err(err) => {
                                client_sender
                                    .send(WebSocketMessage::from(ServerError(Arc::new(err))).into())
                                    .expect("Client sender unexpectedly ended");
                            }
                        }
                    }
                }
                Err(_) => client_sender
                    .send(WebSocketMessage::from(WsStreamEnded).into())
                    .expect("Client sender unexpectedly ended"),
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
                    return Ok(Received(msg).into());
                }
                Err(err) => {
                    error!("{err}");
                    return Ok(WsStreamEnded.into());
                }
            }
        }
        Ok(WsStreamEnded.into())
    }
}
