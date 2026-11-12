use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use niketsu_core::chunk::ChunkRoute;
use niketsu_core::communicator::*;
use p2p::{ChunkRoutes, P2PClient};
use tokio::task::JoinHandle;
use tracing::error;

pub mod messages;
pub mod p2p;

pub const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);
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
    async fn receive(
        &mut self,
        endpoint: &EndpointInfo,
        chunk_routes: &ChunkRoutes,
    ) -> IncomingMessage {
        loop {
            match self {
                Connection::Connected(c) => match c.recv().await {
                    Ok(msg) => return msg,
                    Err(c) => *self = c,
                },
                Connection::Connecting(c) => {
                    *self = c.wait().await;
                }
                Connection::Disconnected(d) => {
                    let reason = d.reason.clone();
                    *self = d.reconnect(endpoint, chunk_routes).await;
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
    async fn recv(&mut self) -> std::result::Result<IncomingMessage, Connection> {
        if let Some(msg) = self.p2p.next().await {
            return Ok(msg);
        }
        Err(Connection::Disconnected(Disconnected::now(Some(
            anyhow::anyhow!("Received None message"),
        ))))
    }

    fn send(&mut self, msg: OutgoingMessage) -> std::result::Result<(), Connection> {
        self.p2p
            .send(msg)
            .inspect_err(|error| error!(%error, "Connection error"))
            .map_err(Some)
            .map_err(Disconnected::now)
            .map_err(Connection::Disconnected)
    }
}

#[derive(Debug)]
pub struct Connecting {
    connect_task: JoinHandle<Result<P2PClient>>,
}

impl Connecting {
    fn new(endpoint: EndpointInfo, chunk_routes: ChunkRoutes) -> Self {
        let connect_task = tokio::task::spawn(async move {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                P2PClient::new(
                    endpoint.addr,
                    endpoint.room,
                    endpoint.password,
                    chunk_routes,
                ),
            )
            .await
            .context("Connection timeout")?
        });
        Self { connect_task }
    }

    async fn wait(&mut self) -> Connection {
        match (&mut self.connect_task).await.map_err(Error::from) {
            Ok(Ok(p2p)) => Connection::Connected(Connected { p2p }),
            Err(error) | Ok(Err(error)) => {
                error!(%error, "Connection error");
                Connection::Disconnected(Disconnected::now(Some(error)))
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

    async fn reconnect(&self, endpoint: &EndpointInfo, chunk_routes: &ChunkRoutes) -> Connection {
        let elapsed = self.when.map(|i| i.elapsed()).unwrap_or(Duration::MAX);
        let remaining = RECONNECT_INTERVAL.saturating_sub(elapsed);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        Connection::Connecting(Connecting::new(endpoint.clone(), chunk_routes.clone()))
    }
}

#[derive(Debug)]
pub struct P2PCommunicator {
    connection: Connection,
    endpoint: Option<EndpointInfo>,
    chunk_routes: ChunkRoutes,
}

impl P2PCommunicator {
    /// The communicator fetches the chunk requests of `fetch` from the
    /// provider, and hands the chunk requests of other peers to `provide`.
    pub fn new(fetch: ChunkRoute, provide: ChunkRoute) -> Self {
        Self {
            connection: Connection::default(),
            endpoint: None,
            chunk_routes: ChunkRoutes { fetch, provide },
        }
    }
}

#[async_trait]
impl CommunicatorTrait for P2PCommunicator {
    fn connect(&mut self, endpoint: EndpointInfo) {
        self.endpoint.replace(endpoint.clone());
        let chunk_routes = self.chunk_routes.clone();
        self.connection = Connection::Connecting(Connecting::new(endpoint, chunk_routes))
    }

    fn send(&mut self, msg: OutgoingMessage) {
        if let Connection::Connected(con) = &mut self.connection
            && let Err(con) = con.send(msg)
        {
            self.connection = con
        }
    }

    async fn receive(&mut self) -> IncomingMessage {
        let Some(endpoint) = &self.endpoint else {
            return std::future::pending().await;
        };
        self.connection.receive(endpoint, &self.chunk_routes).await
    }

    fn has_endpoint(&self) -> bool {
        self.endpoint.is_some()
    }
}

#[cfg(test)]
mod test {}
