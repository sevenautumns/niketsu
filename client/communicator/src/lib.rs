use std::collections::VecDeque;
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures::Future;
use log::{debug, error, warn};
use niketsu_core::communicator::*;
use p2p::P2PClient;
use tokio::task::JoinHandle;

use self::messages::NiketsuMessage;

pub mod messages;
pub mod p2p;

pub const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);

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
    error: Option<anyhow::Error>,
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
        Self {
            last_reconnect,
            error: None,
        }
    }
}

#[derive(Debug)]
struct Connected {
    sender_task: JoinHandle<()>,
    sender_receiver: P2PClient,
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
        let client = tokio::time::timeout(
            Duration::from_secs(5),
            P2PClient::new(endpoint.room, endpoint.password, endpoint.secure),
        );
        let mut sender_receiver = client
            .await
            .map_err(|_| anyhow::anyhow!("Connection timeout"))?
            .map_err(anyhow::Error::from)?;
        let sender_task = sender_receiver.run();
        Ok(Self {
            sender_task,
            sender_receiver,
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
            if let Some(msg) = self.sender_receiver.next().await {
                match std::str::from_utf8(&msg) {
                    Ok(msg) => match serde_json::from_str::<NiketsuMessage>(&msg) {
                        Ok(msg) => return Ok(msg),
                        Err(e) => {
                            error!("msg parse error: {e:?}");
                            continue;
                        }
                    },
                    Err(e) => {
                        error!("from utf8 failed: {e:?}");
                        continue;
                    }
                }
            }
            bail!("Websocket ended")
        }
    }

    fn send(&self, msg: NiketsuMessage) {
        niketsu_core::log!(self.sender_receiver.send(msg))
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
        debug!("attempt connection for: {endpoint}");
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
            Err(err) => {
                niketsu_core::log!(Err::<Connected, _>(&err));
                ConnectionState::Disconnected(Disconnected {
                    error: Some(err),
                    ..Default::default()
                })
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
        if let ConnectionState::Disconnected(disconnected) = &mut self.state {
            if let Some(error) = disconnected.error.take() {
                self.in_queue
                    .push_back(NiketsuConnectionError(error.to_string()).into());
            }
        }

        self.reconnect().await;
        None
    }

    fn disconnect_on_err(&mut self, msg: Result<IncomingMessage>) -> Option<IncomingMessage> {
        match msg {
            Ok(msg) => Some(msg),
            Err(err) => {
                niketsu_core::log!(Err::<Connected, _>(&err));
                self.state = ConnectionState::Disconnected(Disconnected {
                    error: Some(err),
                    ..Default::default()
                });
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
            if let ConnectionState::Disconnected(dis) = &mut self.state {
                if let Some(error) = dis.error.take() {
                    self.in_queue
                        .push_back(NiketsuConnectionError(error.to_string()).into())
                }
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
        self.endpoint = Some(endpoint.clone());
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
