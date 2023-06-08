use std::collections::VecDeque;
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::{Future, SinkExt, StreamExt};
use log::{debug, error, warn};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::task::JoinHandle;
use tokio_native_tls::TlsStream;

use self::messages::NiketsuMessage;
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
enum ConnectionState {
    Disconnected(Disconnected),
    Connected(Connected),
    Connecting(Connecting),
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::Disconnected(Disconnected::default())
    }
}

#[derive(Debug)]
struct Disconnected {
    last_reconnect: Instant,
}

impl Disconnected {
    fn last_reconnect(&self) -> &Instant {
        &self.last_reconnect
    }
}

impl From<Disconnected> for ConnectionState {
    fn from(value: Disconnected) -> Self {
        ConnectionState::Disconnected(value)
    }
}

impl Default for Disconnected {
    fn default() -> Self {
        let last_reconnect = Instant::now();
        Self { last_reconnect }
    }
}

#[derive(Debug)]
struct Connected {
    sender_task: JoinHandle<()>,
    sender: MpscSender<NiketsuMessage>,
    receiver: WsStream,
}

impl Drop for Connected {
    fn drop(&mut self) {
        self.sender_task.abort_handle();
    }
}

impl From<Connected> for ConnectionState {
    fn from(value: Connected) -> Self {
        ConnectionState::Connected(value)
    }
}

impl Connected {
    async fn new(endpoint: EndpointInfo) -> Result<Self> {
        let addr = endpoint.to_string();
        let (sender, rx) = tokio::sync::mpsc::unbounded_channel();
        let (ws, _) = async_tungstenite::tokio::connect_async(addr).await?;
        let (sink, receiver) = ws.split();
        let sender_task = tokio::task::spawn(Self::sender(rx, sink));
        Ok(Self {
            sender_task,
            sender,
            receiver,
        })
    }

    async fn receive_incoming_message(&mut self) -> Result<IncomingMessage> {
        loop {
            let msg = self.receive_niketsu_message().await?;
            if let ping @ NiketsuMessage::Ping(_) = msg {
                self.send(ping);
                continue;
            }
            return msg
                .try_into()
                .map_err(|msg| anyhow!("unexpected message: {msg:?}"));
        }
    }

    async fn receive_niketsu_message(&mut self) -> Result<NiketsuMessage> {
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
        crate::log!(self.sender.send(msg))
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

#[derive(Debug)]
struct Connecting {
    handle: JoinHandle<Result<Connected>>,
}

impl From<Connecting> for ConnectionState {
    fn from(value: Connecting) -> Self {
        ConnectionState::Connecting(value)
    }
}

impl Connecting {
    fn connect(endpoint: EndpointInfo) -> Self {
        debug!("Attempt connection for: {endpoint}");
        let handle = tokio::task::spawn(Connected::new(endpoint));
        Self { handle }
    }

    async fn reconnect(endpoint: EndpointInfo, last_reconnect: &Instant) -> Self {
        let remaining_time = RECONNECT_INTERVAL.saturating_sub(last_reconnect.elapsed());
        if !remaining_time.is_zero() {
            tokio::time::sleep(remaining_time).await;
        }
        Self::connect(endpoint)
    }
}

impl From<Result<Connected>> for ConnectionState {
    fn from(result: Result<Connected>) -> Self {
        match result {
            Ok(connected) => ConnectionState::Connected(connected),
            err @ Err(_) => {
                crate::log!(err);
                ConnectionState::Disconnected(Disconnected::default())
            }
        }
    }
}

impl Future for Connecting {
    type Output = ConnectionState;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let handle = unsafe { self.map_unchecked_mut(|s| &mut s.handle) };
        let poll = handle.poll(cx);
        let Poll::Ready(result) = poll else {
            return Poll::Pending;
        };
        let result = result.unwrap_or_else(|err| Err(anyhow::Error::from(err)));
        Poll::Ready(ConnectionState::from(result))
    }
}

#[derive(Debug, Default)]
pub struct WebsocketCommunicator {
    state: ConnectionState,
    endpoint: Option<EndpointInfo>,
    in_queue: VecDeque<IncomingMessage>,
}

impl WebsocketCommunicator {
    async fn receive_or_reconnect(&mut self) -> Option<IncomingMessage> {
        if let ConnectionState::Connected(connection) = &mut self.state {
            let msg = connection.receive_incoming_message().await;
            return self.disconnect_on_err(msg);
        }

        self.reconnect().await;
        None
    }

    fn disconnect_on_err(&mut self, msg: Result<IncomingMessage>) -> Option<IncomingMessage> {
        match msg {
            Ok(msg) => Some(msg),
            err @ Err(_) => {
                crate::log!(err);
                self.state = Disconnected::default().into();
                None
            }
        }
    }

    async fn reconnect(&mut self) {
        if let ConnectionState::Connecting(connecting) = &mut self.state {
            self.state = connecting.await;
            if let ConnectionState::Connected(_) = &self.state {
                self.in_queue.push_back(NiketsuConnected.into());
            }
            return;
        }
        let Some(endpoint) = self.endpoint.clone() else {
            tokio::time::sleep(Duration::from_secs(300)).await;
            return;
        };
        match &mut self.state {
            ConnectionState::Disconnected(disconnected) => {
                let last_reconnect = disconnected.last_reconnect();
                self.state = Connecting::reconnect(endpoint, last_reconnect).await.into();
            }
            ConnectionState::Connecting(_) => {
                self.state = Connecting::connect(endpoint).into();
            }
            _ => {}
        }
    }
}

#[async_trait]
impl CommunicatorTrait for WebsocketCommunicator {
    fn connect(&mut self, endpoint: EndpointInfo) {
        self.state = Connecting::connect(endpoint).into();
    }

    fn send(&mut self, msg: OutgoingMessage) {
        let ConnectionState::Connected(connection) = &mut self.state else {
            warn!("message dropped: {msg:?}");
            return;
        };
        connection.send(msg.into())
    }

    async fn receive(&mut self) -> IncomingMessage {
        loop {
            if let Some(msg) = self.in_queue.pop_front() {
                return msg;
            }
            if let Some(msg) = self.receive_or_reconnect().await {
                return msg;
            }
        }
    }
}

#[cfg(test)]
mod test {}
