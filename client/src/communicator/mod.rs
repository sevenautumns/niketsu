use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use async_trait::async_trait;
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use log::{debug, error, warn};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::task::JoinHandle;
use tokio_native_tls::TlsStream;

use self::messages::{NiketsuMessage, SeekMessage};
use crate::client::LogResult;
use crate::core::communicator::*;

pub mod messages;

type WsSink = SplitSink<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
    TsMessage,
>;
type WsStream = SplitStream<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
>;

pub const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct WebsocketCommunicator {
    // TODO remove with resolution of issue #83
    last_filename: String,
    // TODO remove with resolution of issue #83
    last_paused_state: bool,

    addr: String,
    last_reconnect: Instant,
    reconnect: Option<JoinHandle<Result<WebsocketConnection>>>,
    connection: Option<WebsocketConnection>,
    in_queue: VecDeque<IncomingMessage>,
}

impl WebsocketCommunicator {
    async fn await_reconnect_handle(&mut self) -> Result<()> {
        let Some(handle) = &mut self.reconnect else {
            bail!("No handle found");
        };
        let res = handle.await;
        self.reconnect.take();
        self.last_reconnect = Instant::now();
        self.connection = Some(res??);
        self.in_queue.push_back(NiketsuConnected.into());
        Ok(())
    }

    async fn reconnect_attempt(&mut self) -> Result<()> {
        self.connection = None;
        if self.reconnect.is_some() {
            return self.await_reconnect_handle().await;
        }

        tokio::time::sleep(self.time_until_next_reconnect()).await;
        self.create_reconnct();
        self.await_reconnect_handle().await
    }

    fn create_reconnct(&mut self) {
        self.reconnect = Some(tokio::task::spawn(WebsocketConnection::new(
            self.addr.clone(),
        )));
    }

    fn time_until_next_reconnect(&self) -> Duration {
        self.last_reconnect
            .elapsed()
            .saturating_sub(RECONNECT_INTERVAL)
    }

    // TODO remove this when #83 lands and implement a general `From` instead of this
    fn outgoing_to_niketsu_message(&mut self, msg: OutgoingMessage) -> NiketsuMessage {
        match msg {
            OutgoingMessage::Join(m) => m.into(),
            OutgoingMessage::VideoStatus(m) => {
                self.last_filename = m.filename.clone().unwrap_or_default();
                self.last_paused_state = m.paused;
                m.into()
            }
            OutgoingMessage::Start(m) => {
                self.last_paused_state = false;
                m.into()
            }
            OutgoingMessage::Pause(m) => {
                self.last_paused_state = true;
                m.into()
            }
            OutgoingMessage::PlaybackSpeed(m) => m.into(),
            OutgoingMessage::Seek(m) => NiketsuMessage::Seek(SeekMessage::new(
                m,
                self.last_filename.clone(),
                self.last_paused_state,
            )),
            OutgoingMessage::Select(m) => {
                self.last_filename = m.filename.clone().unwrap_or_default();
                m.into()
            }
            OutgoingMessage::UserMessage(m) => m.into(),
            OutgoingMessage::Playlist(m) => m.into(),
            OutgoingMessage::UserStatus(m) => m.into(),
        }
    }
}

#[async_trait]
impl CommunicatorTrait for WebsocketCommunicator {
    fn new(addr: String) -> Self {
        let mut com = Self {
            last_filename: Default::default(),
            last_paused_state: true,
            in_queue: VecDeque::new(),
            addr,
            last_reconnect: Instant::now(),
            reconnect: None,
            connection: None,
        };
        com.create_reconnct();
        com
    }

    fn send(&mut self, msg: OutgoingMessage) {
        let msg = self.outgoing_to_niketsu_message(msg);
        let Some(conn) = &self.connection else {
            warn!("message dropped: {msg:?}");
            return;
        };
        conn.send(msg);
    }

    async fn receive(&mut self) -> IncomingMessage {
        // TODO remove with resolution of issue #83
        if let Some(msg) = self.in_queue.pop_front() {
            return msg;
        }
        loop {
            if let Some(conn) = &mut self.connection {
                match conn.receive().await {
                    Ok(msg) => {
                        if let ping @ NiketsuMessage::Ping(_) = msg {
                            conn.send(ping);
                            continue;
                        }
                        // TODO remove with resolution of issue #83
                        if let NiketsuMessage::Seek(seek) = msg {
                            self.last_filename = seek.filename.clone();
                            self.last_paused_state = seek.paused;
                            for msg in seek.into_incoming_message() {
                                self.in_queue.push_back(msg);
                            }
                            return self
                                .in_queue
                                .pop_front()
                                .expect("Left over messages are empty");
                        }
                        if let NiketsuMessage::Select(select) = &msg {
                            self.last_filename = select.filename.clone().unwrap_or_default();
                        }
                        if let NiketsuMessage::Pause(_) = &msg {
                            self.last_paused_state = true;
                        }
                        if let NiketsuMessage::Start(_) = &msg {
                            self.last_paused_state = false;
                        }
                        match msg.try_into() {
                            Ok(msg) => return msg,
                            Err(msg) => warn!("unexpected message: {msg:?}"),
                        }
                    }
                    Err(err) => error!("server error: {err:?}"),
                }
            }

            self.reconnect_attempt().await.log();
        }
    }
}

#[derive(Debug)]
struct WebsocketConnection {
    _sender_task: JoinHandle<()>,
    sender: MpscSender<NiketsuMessage>,
    receiver: WsStream,
}

impl WebsocketConnection {
    async fn new(addr: String) -> Result<Self> {
        let (sender, rx) = tokio::sync::mpsc::unbounded_channel();
        let (ws, _) = async_tungstenite::tokio::connect_async(addr).await?;
        let (sink, receiver) = ws.split();
        let sender_task = tokio::task::spawn(Self::sender(rx, sink));
        Ok(Self {
            _sender_task: sender_task,
            sender,
            receiver,
        })
    }

    async fn receive(&mut self) -> Result<NiketsuMessage> {
        loop {
            if let Some(msg) = self.receiver.next().await {
                match msg?.into_text() {
                    Ok(msg) => match serde_json::from_str::<NiketsuMessage>(&msg) {
                        Ok(msg) => return Ok(msg),
                        Err(e) => {
                            error!("msg parse error: {e:?}");
                            continue;
                        }
                    },
                    Err(e) => {
                        error!("{e:?}");
                        continue;
                    }
                }
            }
            bail!("Websocket ended")
        }
    }

    fn send(&self, msg: NiketsuMessage) {
        if let Err(e) = self.sender.send(msg) {
            error!("send error: {e:?}")
        }
    }

    async fn sender(mut ch: MpscReceiver<NiketsuMessage>, mut sink: WsSink) {
        loop {
            if let Some(msg) = ch.recv().await {
                debug!("Sending {msg:?}");
                match serde_json::to_string(&msg) {
                    Ok(msg) => {
                        if let Err(err) = sink.send(TsMessage::Text(msg)).await {
                            warn!("Websocket ended: {err:?}");
                            return;
                        }
                    }
                    Err(err) => {
                        warn!("serde error: {err:?}")
                    }
                }
            } else {
                return;
            }
        }
    }
}
