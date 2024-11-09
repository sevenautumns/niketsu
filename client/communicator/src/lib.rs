use std::future::Future;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

use anyhow::{Error, Result};
use async_trait::async_trait;
use niketsu_core::communicator::*;
use p2p::P2PClient;
use tokio::task::JoinHandle;
use tracing::{error, warn};

use self::messages::NiketsuMessage;

pub mod messages;
pub mod p2p;

pub const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum Connection {
    Connected(Connected),
    Connecting(Connecting),
    Disconnected(Disconnected),
}

impl Default for Connection {
    fn default() -> Self {
        Self::Disconnected(Disconnected::default())
    }
}

impl Connection {
    async fn receive(&mut self, endpoint: &EndpointInfo) -> IncomingMessage {
        loop {
            match self {
                Connection::Connected(c) => match c.recv().await.map(IncomingMessage::try_from) {
                    Ok(Ok(msg)) => return msg,
                    Ok(Err(msg)) => warn!(?msg, "received unexpected message"),
                    Err(c) => *self = c,
                },
                Connection::Connecting(c) => {
                    *self = c.await;
                }
                Connection::Disconnected(d) => {
                    let reason = d.reason.clone();
                    *self = d.reconnect(endpoint).await;
                    if let Some(r) = reason {
                        return IncomingMessage::from(ServerMessageMsg {
                            message: r.to_string(),
                        });
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Connected {
    p2p: P2PClient,
}

impl Connected {
    async fn recv(&mut self) -> std::result::Result<NiketsuMessage, Connection> {
        if let Some(msg) = self.p2p.next().await {
            return Ok(msg);
        }
        Err(Connection::Disconnected(Disconnected::now(Some(
            anyhow::anyhow!("Received None message"),
        ))))
    }

    fn send(&mut self, msg: NiketsuMessage) -> std::result::Result<(), Connection> {
        if let Err(error) = self.p2p.send(msg) {
            error!(%error, "Connection error");
            return Err(Connection::Disconnected(Disconnected::now(Some(error))));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Connecting {
    connect_task: JoinHandle<Result<P2PClient>>,
}

impl Connecting {
    fn new(endpoint: EndpointInfo) -> Self {
        let connection = tokio::time::timeout(
            CONNECT_TIMEOUT,
            P2PClient::new(
                endpoint.addr.clone(),
                endpoint.room.clone(),
                endpoint.password.clone(),
            ),
        );
        let connect_task = tokio::task::spawn(async move {
            match connection.await {
                Ok(res) => match res {
                    Ok(client) => return Ok(client),
                    Err(err) => return Err(err),
                },
                Err(err) => return Err(anyhow::anyhow!("Connection timeout: {}", err)),
            }
        });
        Self { connect_task }
    }
}

impl Future for Connecting {
    type Output = Connection;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let pin = unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.connect_task) };
        let Poll::Ready(p2p) = pin.poll(cx) else {
            return Poll::Pending;
        };
        let p2p = p2p.map_err(anyhow::Error::from);
        match p2p {
            Ok(Ok(p2p)) => Poll::Ready(Connection::Connected(Connected { p2p })),
            Err(error) | Ok(Err(error)) => {
                error!(%error, "Connection error");
                Poll::Ready(Connection::Disconnected(Disconnected::now(Some(error))))
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Disconnected {
    when: Option<Instant>,
    reason: Option<Arc<Error>>,
}

impl Disconnected {
    fn now(reason: Option<Error>) -> Self {
        Self {
            when: Some(Instant::now()),
            reason: reason.map(Arc::new),
        }
    }

    async fn reconnect(&self, endpoint: &EndpointInfo) -> Connection {
        let elapsed = self.when.map(|i| i.elapsed()).unwrap_or(Duration::MAX);
        let remaining = RECONNECT_INTERVAL.saturating_sub(elapsed);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        Connection::Connecting(Connecting::new(endpoint.clone()))
    }
}

#[derive(Debug, Default)]
pub struct P2PCommunicator {
    connection: Connection,
    endpoint: Option<EndpointInfo>,
}

#[async_trait]
impl CommunicatorTrait for P2PCommunicator {
    fn connect(&mut self, endpoint: EndpointInfo) {
        self.endpoint.replace(endpoint.clone());
        self.connection = Connection::Connecting(Connecting::new(endpoint))
    }

    fn send(&mut self, msg: OutgoingMessage) {
        if let Connection::Connected(con) = &mut self.connection {
            if let Err(con) = con.send(msg.into()) {
                self.connection = con
            }
        }
    }

    async fn receive(&mut self) -> IncomingMessage {
        let Some(endpoint) = &self.endpoint else {
            return std::future::pending().await;
        };
        self.connection.receive(endpoint).await
    }
}

#[cfg(test)]
mod test {}
