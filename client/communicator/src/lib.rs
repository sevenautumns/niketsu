use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use backoff::Backoff;
use libp2p::identity::Keypair;
use niketsu_core::chunk::{FetchRoute, ProvideRoute};
use niketsu_core::communicator::*;
use p2p::{AuthDenied, ChunkRoutes, SessionHandle};
use timing::CONNECT_TIMEOUT;
use tokio::task::JoinHandle;
use tracing::error;

mod backoff;
pub mod messages;
pub mod p2p;
mod timing;

#[derive(Debug)]
pub enum Connection {
    Connected(Connected),
    Connecting(Connecting),
    Disconnected(Disconnected),
    Denied(Denied),
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
                    if let Some(reason) = reason {
                        let reason = reason.to_string();
                        return DisconnectedMsg { reason }.into();
                    }
                }
                Connection::Denied(d) => {
                    return match d.reason.take() {
                        Some(reason) => {
                            let reason = reason.to_string();
                            DisconnectedMsg { reason }.into()
                        }
                        None => std::future::pending().await,
                    };
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
    /// An established connection that ends is replaced promptly, so the
    /// next attempt waits no more than the reconnect interval.
    async fn recv(&mut self) -> std::result::Result<IncomingMessage, Connection> {
        self.session.next().await.map_err(|stop| {
            Connection::Disconnected(Disconnected::now(
                Some(anyhow::anyhow!("{stop}")),
                Backoff::default(),
            ))
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
    /// How long to wait for the next attempt should this one fail.
    backoff: Backoff,
}

impl Connecting {
    fn new(endpoint: EndpointInfo, node: Node, backoff: Backoff) -> Self {
        let connect_task = tokio::task::spawn(async move {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                SessionHandle::new(node, endpoint.addr, endpoint.room, endpoint.password),
            )
            .await
            .context("Connection timeout")?
        });
        Self {
            connect_task,
            backoff,
        }
    }

    async fn wait(&mut self) -> Connection {
        match (&mut self.connect_task).await.map_err(Error::from) {
            Ok(Ok(session)) => Connection::Connected(Connected { session }),
            Err(error) | Ok(Err(error)) => {
                error!(%error, "Connection error");
                if error.is::<AuthDenied>() {
                    Connection::Denied(Denied {
                        reason: Some(error),
                    })
                } else {
                    Connection::Disconnected(Disconnected::now(Some(error), self.backoff))
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Disconnected {
    when: Option<Instant>,
    reason: Option<Arc<Error>>,
    backoff: Backoff,
}

impl Disconnected {
    fn now(reason: Option<Error>, backoff: Backoff) -> Self {
        Self {
            when: Some(Instant::now()),
            reason: reason.map(Arc::new),
            backoff,
        }
    }

    async fn reconnect(&self, endpoint: &EndpointInfo, node: &Node) -> Connection {
        let elapsed = self.when.map(|i| i.elapsed()).unwrap_or(Duration::MAX);
        let remaining = self.backoff.delay().saturating_sub(elapsed);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        Connection::Connecting(Connecting::new(
            endpoint.clone(),
            node.clone(),
            self.backoff.next(),
        ))
    }
}

/// The relay denied the room. Asking again would get the same answer, so
/// the connection stays down until the core connects anew.
#[derive(Debug)]
pub struct Denied {
    /// Why, until the user has been told.
    reason: Option<Error>,
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
    /// peers they name, and answers the chunk requests of other peers
    /// from `provide`.
    pub fn new(fetch: FetchRoute, provide: ProvideRoute) -> Self {
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
    fn peer(&self) -> PeerId {
        self.node.keypair.public().to_peer_id()
    }

    fn connect(&mut self, endpoint: EndpointInfo) {
        self.endpoint.replace(endpoint.clone());
        self.connection = Connection::Connecting(Connecting::new(
            endpoint,
            self.node.clone(),
            Backoff::default(),
        ))
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

    fn communicator() -> P2PCommunicator {
        let mut communicator = P2PCommunicator::new(FetchRoute::default(), ProvideRoute::default());
        communicator.endpoint = Some(EndpointInfo {
            addr: "/ip4/127.0.0.1/udp/1/quic-v1".parse().unwrap(),
            room: "room".into(),
            password: String::new(),
        });
        communicator
    }

    /// An attempt that has already ended with `outcome`.
    fn attempt(outcome: Result<SessionHandle>, backoff: Backoff) -> Connecting {
        Connecting {
            connect_task: tokio::spawn(async { outcome }),
            backoff,
        }
    }

    /// After a denial the user is told once, and no attempt follows.
    #[tokio::test]
    async fn a_denied_room_is_not_asked_for_again() {
        let mut communicator = communicator();
        communicator.connection =
            Connection::Connecting(attempt(Err(AuthDenied.into()), Backoff::default()));

        let IncomingMessage::Disconnected(told) = communicator.receive().await else {
            panic!("the user was not told of the denial");
        };
        assert_eq!(told.reason, AuthDenied.to_string());

        for _ in 0..2 {
            let waited =
                tokio::time::timeout(Backoff::default().next().delay(), communicator.receive())
                    .await;
            assert!(waited.is_err(), "a denied connection said more");
            assert!(matches!(communicator.connection, Connection::Denied(_)));
        }
    }

    /// Connecting again leaves the denial behind.
    #[tokio::test]
    async fn connecting_again_after_a_denial_makes_a_new_attempt() {
        let mut communicator = communicator();
        communicator.connection = Connection::Denied(Denied { reason: None });
        let endpoint = communicator.endpoint.clone().unwrap();
        communicator.connect(endpoint);
        assert!(matches!(communicator.connection, Connection::Connecting(_)));
    }

    /// A failure that may pass waits as long as its attempt's backoff says.
    #[tokio::test]
    async fn a_failed_attempt_hands_on_its_backoff() {
        let backoff = Backoff::default().next().next();
        let mut connecting = attempt(Err(anyhow::anyhow!("relay unreachable")), backoff);
        let Connection::Disconnected(disconnected) = connecting.wait().await else {
            panic!("a failure that may pass ended the attempts");
        };
        assert_eq!(disconnected.backoff, backoff);
        assert!(disconnected.reason.is_some());
    }

    /// Losing an established connection retries at the base interval, however
    /// long the attempts before it waited.
    #[tokio::test]
    async fn a_connection_resets_the_backoff() {
        let backoff = Backoff::default().next().next().next();
        let mut connecting = attempt(Ok(SessionHandle::ended_with(vec![])), backoff);
        let Connection::Connected(mut connected) = connecting.wait().await else {
            panic!("the attempt did not connect");
        };
        let Err(Connection::Disconnected(disconnected)) = connected.recv().await else {
            panic!("the ended session did not disconnect");
        };
        assert_eq!(disconnected.backoff, Backoff::default());
    }

    /// What the room said before the connection ended still reaches the
    /// core, also when the core finds the connection ended by sending.
    #[tokio::test]
    async fn messages_of_an_ended_connection_survive_a_failed_send() {
        let last_words = LineMsg {
            actor: "bob".into(),
            message: "bye".into(),
            kind: LineKind::Action,
        };
        let mut communicator = communicator();
        communicator.connection = Connection::Connected(Connected {
            session: SessionHandle::ended_with(vec![last_words.clone().into()]),
        });

        communicator.send(Intent::Pause.into());

        let IncomingMessage::Line(received) = communicator.receive().await else {
            panic!("the message was dropped with the connection");
        };
        assert_eq!(received, last_words);
    }
}
