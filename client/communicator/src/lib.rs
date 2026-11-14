use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use libp2p::identity::Keypair;
use niketsu_core::chunk::ChunkRoute;
use niketsu_core::communicator::*;
use p2p::{ChunkRoutes, SessionHandle};
use timing::{CONNECT_TIMEOUT, RECONNECT_INTERVAL};
use tokio::task::JoinHandle;
use tracing::error;

pub mod messages;
pub mod p2p;
mod timing;

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
    async fn receive(&mut self, endpoint: &EndpointInfo, node: &Node) -> IncomingMessage {
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
                    *self = d.reconnect(endpoint, node).await;
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
    session: SessionHandle,
}

impl Connected {
    async fn recv(&mut self) -> std::result::Result<IncomingMessage, Connection> {
        self.session.next().await.map_err(|stop| {
            Connection::Disconnected(Disconnected::now(Some(anyhow::anyhow!("{stop}"))))
        })
    }

    /// A send fails once the connection's event loop has ended. The messages
    /// it handed over before that are still to be received, so the
    /// connection stays until `recv` finds it drained.
    fn send(&mut self, msg: OutgoingMessage) {
        if let Err(error) = self.session.send(msg) {
            error!(%error, "Connection error");
        }
    }
}

#[derive(Debug)]
pub struct Connecting {
    connect_task: JoinHandle<Result<SessionHandle>>,
}

impl Connecting {
    fn new(endpoint: EndpointInfo, node: Node) -> Self {
        let connect_task = tokio::task::spawn(async move {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                SessionHandle::new(node, endpoint.addr, endpoint.room, endpoint.password),
            )
            .await
            .context("Connection timeout")?
        });
        Self { connect_task }
    }

    async fn wait(&mut self) -> Connection {
        match (&mut self.connect_task).await.map_err(Error::from) {
            Ok(Ok(session)) => Connection::Connected(Connected { session }),
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

    async fn reconnect(&self, endpoint: &EndpointInfo, node: &Node) -> Connection {
        let elapsed = self.when.map(|i| i.elapsed()).unwrap_or(Duration::MAX);
        let remaining = RECONNECT_INTERVAL.saturating_sub(elapsed);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        Connection::Connecting(Connecting::new(endpoint.clone(), node.clone()))
    }
}

/// What every connection of one communicator has in common.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// The identity is the communicator's, so a reconnect keeps the peer id.
    pub(crate) keypair: Keypair,
    /// Whether peers on the local network are discovered through mDNS.
    pub(crate) mdns: bool,
    pub(crate) chunk_routes: ChunkRoutes,
}

#[derive(Debug)]
pub struct P2PCommunicator {
    connection: Connection,
    endpoint: Option<EndpointInfo>,
    node: Node,
}

impl P2PCommunicator {
    /// The communicator fetches the chunk requests of `fetch` from the
    /// provider, and hands the chunk requests of other peers to `provide`.
    pub fn new(fetch: ChunkRoute, provide: ChunkRoute) -> Self {
        Self {
            connection: Connection::default(),
            endpoint: None,
            node: Node {
                keypair: Keypair::generate_ed25519(),
                mdns: true,
                chunk_routes: ChunkRoutes { fetch, provide },
            },
        }
    }

    /// Reaches peers through the relay only, leaving the local network alone.
    pub fn without_mdns(mut self) -> Self {
        self.node.mdns = false;
        self
    }
}

#[async_trait]
impl CommunicatorTrait for P2PCommunicator {
    fn connect(&mut self, endpoint: EndpointInfo) {
        self.endpoint.replace(endpoint.clone());
        self.connection = Connection::Connecting(Connecting::new(endpoint, self.node.clone()))
    }

    fn send(&mut self, msg: OutgoingMessage) {
        if let Connection::Connected(con) = &mut self.connection {
            con.send(msg)
        }
    }

    async fn receive(&mut self) -> IncomingMessage {
        let Some(endpoint) = &self.endpoint else {
            return std::future::pending().await;
        };
        self.connection.receive(endpoint, &self.node).await
    }

    fn has_endpoint(&self) -> bool {
        self.endpoint.is_some()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The room's last words to a peer can be the ones that matter most: a
    /// handover is announced right before the old host closes the room.
    #[tokio::test]
    async fn messages_of_an_ended_connection_survive_a_failed_send() {
        let handover = HostHandoverMsg {
            new_host: "bob".into(),
        };
        let mut communicator = P2PCommunicator::new(ChunkRoute::default(), ChunkRoute::default());
        communicator.endpoint = Some(EndpointInfo {
            addr: "/ip4/127.0.0.1/udp/1/quic-v1".parse().unwrap(),
            room: "room".into(),
            password: String::new(),
        });
        communicator.connection = Connection::Connected(Connected {
            session: SessionHandle::ended_with(vec![handover.clone().into()]),
        });

        communicator.send(VideoShareMsg { video: None }.into());

        let IncomingMessage::HostHandover(received) = communicator.receive().await else {
            panic!("the handover was dropped with the connection");
        };
        assert_eq!(received, handover);
    }
}
