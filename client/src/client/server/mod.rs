use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use iced::widget::{Row, Text};
use iced::{Renderer, Theme};
use log::{error, warn};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio_native_tls::TlsStream;
use url::Url;

use self::message::{Connected, Received, WebSocketMessage, WsStreamEnded};
use crate::client::server::message::ServerError;
use crate::iced_window::MainMessage;
use crate::user::ThisUser;

pub mod message;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum NiketsuMessage {
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

type WsSink = SplitSink<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
    TsMessage,
>;
type WsStream = SplitStream<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
>;

#[derive(Debug)]
pub struct ServerConnectionReceiver {
    addr: Url,
    stream_proxy_rx: MpscReceiver<WebSocketMessage>,
    stream_proxy_tx: MpscSender<WebSocketMessage>,
    sender: ServerConnectionSender,
}

impl ServerConnectionReceiver {
    #[must_use]
    pub fn reboot(&self) -> Self {
        // TODO only reboot if msg_sender is closed?
        Self::new(self.addr.clone())
    }

    pub fn new(addr: Url) -> Self {
        let (sink_proxy_tx, sink_proxy_rx): (MpscSender<NiketsuMessage>, _) =
            tokio::sync::mpsc::unbounded_channel();
        let (stream_proxy_tx, stream_proxy_rx): (MpscSender<WebSocketMessage>, _) =
            tokio::sync::mpsc::unbounded_channel();
        let sender = ServerConnectionSender {
            sink_proxy_tx,
            stream_proxy_tx: stream_proxy_tx.clone(),
        };

        Self {
            addr,
            stream_proxy_rx,
            stream_proxy_tx,
            sender,
        }
        .start(sink_proxy_rx)
    }

    fn start(self, sink_proxy_rx: MpscReceiver<NiketsuMessage>) -> Self {
        let addr = self.addr.clone();
        let sender = self.sender.clone();
        let stream_proxy_tx = self.stream_proxy_tx.clone();
        tokio::spawn(async move {
            let connection = async_tungstenite::tokio::connect_async(addr).await;
            match connection {
                Ok((ws, _)) => {
                    stream_proxy_tx
                        .send(WebSocketMessage::from(Connected))
                        .expect("Client sender unexpectedly ended");
                    let (socket_sink, socket_stream) = ws.split();
                    let send_sender = sender.clone();
                    tokio::spawn(
                        async move { send_sender.run_send(socket_sink, sink_proxy_rx).await },
                    );
                    tokio::spawn(async move { sender.run_recv(socket_stream).await });
                }
                Err(_) => stream_proxy_tx
                    .send(WebSocketMessage::from(WsStreamEnded))
                    .expect("Client sender unexpectedly ended"),
            }
        });

        self
    }

    pub async fn recv(&mut self) -> Result<WebSocketMessage> {
        self.stream_proxy_rx
            .recv()
            .await
            .ok_or_else(|| anyhow!("Socket ended"))
    }

    pub fn sender(&self) -> &ServerConnectionSender {
        &self.sender
    }
}

#[derive(Debug, Clone)]
pub struct ServerConnectionSender {
    sink_proxy_tx: MpscSender<NiketsuMessage>,
    stream_proxy_tx: MpscSender<WebSocketMessage>,
}

impl ServerConnectionSender {
    async fn run_send(
        &self,
        mut socket_sink: WsSink,
        mut sink_proxy_rx: MpscReceiver<NiketsuMessage>,
    ) {
        loop {
            if let Some(msg) = sink_proxy_rx.recv().await {
                match serde_json::to_string(&msg) {
                    Ok(msg) => {
                        if let Err(err) = socket_sink.send(TsMessage::Text(msg)).await {
                            warn!("Websocket ended: {err:?}");
                            self.stream_proxy_tx
                                .send(WebSocketMessage::from(ServerError(Arc::new(err.into()))))
                                .expect("Client sender unexpectedly ended");
                            return;
                        }
                    }
                    Err(err) => self
                        .stream_proxy_tx
                        .send(WebSocketMessage::from(ServerError(Arc::new(err.into()))))
                        .expect("Client sender unexpectedly ended"),
                }
            } else {
                return;
            }
        }
    }

    // TODO move run_send and run_recv to their own struct
    async fn run_recv(&self, mut socket_stream: WsStream) {
        loop {
            match Self::recv_stream(&mut socket_stream).await {
                Ok(msg @ WebSocketMessage::WsStreamEnded(_)) => {
                    self.stream_proxy_tx
                        .send(msg)
                        .expect("Client sender unexpectedly ended");
                    return;
                }
                Ok(msg) => {
                    self.stream_proxy_tx
                        .send(msg)
                        .expect("Client sender unexpectedly ended");
                }
                Err(err) => {
                    self.stream_proxy_tx
                        .send(WebSocketMessage::from(ServerError(Arc::new(err))))
                        .expect("Client sender unexpectedly ended");
                }
            }
        }
    }

    async fn recv_stream(stream: &mut WsStream) -> Result<WebSocketMessage> {
        if let Some(msg) = stream.next().await {
            match msg {
                Ok(msg) => {
                    let msg = msg.into_text()?;
                    let msg = serde_json::from_str::<NiketsuMessage>(&msg)?;
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

    pub fn send(&self, msg: NiketsuMessage) -> Result<()> {
        self.sink_proxy_tx.send(msg)?;
        Ok(())
    }
}
