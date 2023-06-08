use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use enum_dispatch::enum_dispatch;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use iced::widget::{Row, Text};
use iced::{Renderer, Theme};
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio_native_tls::TlsStream;
use url::Url;

use self::message::{Connected, WebSocketMessage, WsStreamEnded};
use super::CoreRunner;
use crate::client::message::CoreMessageTrait;
use crate::client::server::message::ServerError;
use crate::client::LogResult;
use crate::iced_window::MainMessage;
use crate::playlist::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::user::ThisUser;
use crate::video::{PlayingFile, Video};

pub mod message;

#[enum_dispatch(CoreMessageTrait)]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum NiketsuMessage {
    Ping(NiketsuPing),
    Join(NiketsuJoin),
    VideoStatus(NiketsuVideoStatus),
    StatusList(NiketsuStatusList),
    Pause(NiketsuPause),
    Start(NiketsuStart),
    PlaybackSpeed(NiketsuPlaybackSpeed),
    Seek(NiketsuSeek),
    Select(NiketsuSelect),
    UserMessage(NiketsuUserMessage),
    ServerMessage(NiketsuServerMessage),
    Playlist(NiketsuPlaylist),
    Status(NiketsuStatus),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuPing {
    uuid: String,
}

impl CoreMessageTrait for NiketsuPing {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.ws.sender().send(self)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuJoin {
    pub password: String,
    pub room: String,
    pub username: String,
}

impl CoreMessageTrait for NiketsuJoin {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        warn!("Received: {self:?}");
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuVideoStatus {
    pub filename: Option<String>,
    #[serde(with = "serde_millis")]
    pub position: Option<Duration>,
    pub speed: f64,
    pub paused: bool,
}

impl CoreMessageTrait for NiketsuVideoStatus {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuStatusList {
    pub rooms: BTreeMap<String, BTreeSet<NiketsuUserStatus>>,
}

impl CoreMessageTrait for NiketsuStatusList {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.rooms_widget.rcu(|r| {
            let mut rs = RoomsWidgetState::clone(r);
            rs.replace_rooms(self.rooms.clone());
            rs
        });
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuPause {
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuPause {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.player.pause()?;
        client.messages.push_paused(self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuStart {
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuStart {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.player.start()?;
        client.messages.push_started(self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuPlaybackSpeed {
    pub speed: f64,
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuPlaybackSpeed {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.player.set_speed(self.speed).log();
        client
            .messages
            .push_playback_speed(self.speed, self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuSeek {
    pub filename: String,
    #[serde(with = "serde_millis")]
    pub position: Duration,
    #[serde(skip_serializing)]
    pub username: String,
    pub paused: bool,
    pub speed: f64,
    #[serde(skip_serializing)]
    pub desync: bool,
}

impl CoreMessageTrait for NiketsuSeek {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        if !client.player.is_seeking()? {
            client.player.load(self.borrow().into()).log();
            client
                .messages
                .push_seek(self.position, self.filename, self.desync, self.username);
        }
        Ok(())
    }
}

impl From<&NiketsuSeek> for PlayingFile {
    fn from(seek: &NiketsuSeek) -> Self {
        PlayingFile {
            video: Video::from_string(seek.filename.clone()),
            paused: seek.paused,
            speed: seek.speed,
            pos: seek.position,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuSelect {
    pub filename: Option<String>,
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuSelect {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        match self.filename.clone() {
            Some(filename) => client
                .player
                .load(PlayingFile {
                    video: Video::from_string(filename),
                    paused: true,
                    speed: client.player.get_speed()?,
                    pos: Duration::ZERO,
                })
                .log(),
            None => client.player.unload(),
        }
        client.messages.push_select(self.filename, self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuUserMessage {
    pub message: String,
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuUserMessage {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.messages.push_user_chat(self.message, self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuServerMessage {
    pub message: String,
    pub error: bool,
}

impl CoreMessageTrait for NiketsuServerMessage {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.messages.push_server_chat(self.message, self.error);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuPlaylist {
    pub playlist: Vec<String>,
    #[serde(skip_serializing)]
    pub username: String,
}

impl CoreMessageTrait for NiketsuPlaylist {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Received: {self:?}");
        client.playlist_widget.rcu(|p| {
            let mut plist = PlaylistWidgetState::clone(p);
            plist.replace_videos(self.playlist.clone());
            plist
        });
        client.messages.push_playlist_changed(self.username);
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuStatus {
    pub ready: bool,
    pub username: String,
}

impl CoreMessageTrait for NiketsuStatus {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        warn!("Received: {self:?}");
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NiketsuUserStatus {
    pub username: String,
    pub ready: bool,
}

impl PartialEq for NiketsuUserStatus {
    fn eq(&self, other: &Self) -> bool {
        self.username.eq(&other.username)
    }
}

impl Ord for NiketsuUserStatus {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.username.cmp(&other.username)
    }
}
impl PartialOrd for NiketsuUserStatus {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.username.partial_cmp(&other.username)
    }
}

impl NiketsuUserStatus {
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
                debug!("Sending {msg:?}");
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
                    return Ok(msg.into());
                }
                Err(err) => {
                    error!("{err}");
                    return Ok(WsStreamEnded.into());
                }
            }
        }
        Ok(WsStreamEnded.into())
    }

    pub fn send<M: Into<NiketsuMessage>>(&self, msg: M) -> Result<()> {
        self.sink_proxy_tx.send(msg.into())?;
        Ok(())
    }
}
