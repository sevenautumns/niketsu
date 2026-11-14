use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use libp2p::core::transport::ListenerId;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, gossipsub, identify};
use niketsu_core::communicator::{IncomingMessage, OutgoingMessage};
use niketsu_core::room::RoomName;
use tracing::{debug, info, warn};

use super::auth::{AuthDenied, AuthEvent};
use super::client::ClientRole;
use super::host::HostRole;
use super::member::Membership;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, Role, SwarmExt, TransportBehaviourEvent, host_relay,
};
use crate::timing::LISTENER_PATIENCE;

struct PeerInfo {
    relay: PeerId,
    host: Option<PeerId>,
}

/// Takes a fresh swarm into the room: listens, authenticates with the
/// relay, and learns from it which role this peer has there.
pub(crate) struct Setup {
    swarm: Swarm<Behaviour>,
    relay_addr: Multiaddr,
    room: RoomName,
    password: String,
    topic: gossipsub::IdentTopic,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
    chunk_routes: ChunkRoutes,
}

impl Setup {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
        topic: gossipsub::IdentTopic,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        Self {
            swarm,
            relay_addr,
            room,
            password,
            topic,
            core_receiver,
            message_sender,
            chunk_routes,
        }
    }

    pub(crate) async fn run(mut self) -> Result<Box<dyn Role>> {
        let listeners = [
            "/ip4/0.0.0.0/udp/0/quic-v1",
            "/ip4/0.0.0.0/tcp/0",
            "/ip6/::/udp/0/quic-v1",
            "/ip6/::/tcp/0",
        ]
        .into_iter()
        .map(|addr| Ok(self.swarm.listen_on(addr.parse()?)?))
        .collect::<Result<HashSet<ListenerId>>>()?;
        self.await_listeners(listeners).await;

        info!("Dialing relay");
        self.swarm
            .dial(self.relay_addr.clone())
            .context("Failed to dial relay")?;

        let peer_info = self.connect().await?;
        let local_peer_id = *self.swarm.local_peer_id();

        let relay_with_peer = self
            .relay_addr
            .clone()
            .with_p2p(peer_info.relay)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        self.swarm.gossipsub().subscribe(&self.topic)?;
        let membership = Membership::new(self.room, self.password, &local_peer_id);

        let role: Box<dyn Role> = match peer_info.host {
            Some(host_peer_id) => {
                info!(%host_peer_id, "Dialing host via relay circuit");
                let peer_addr = relay_with_peer
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(host_peer_id));
                self.swarm.dial(peer_addr)?;
                Box::new(ClientRole::new(
                    self.swarm,
                    self.topic,
                    membership,
                    host_peer_id,
                    self.core_receiver,
                    self.message_sender,
                    self.chunk_routes,
                ))
            }
            None => {
                info!(%local_peer_id, "Listening on relay circuit as host");
                let reservation = self
                    .swarm
                    .listen_on(relay_with_peer.with(Protocol::P2pCircuit))
                    .context("Failed to listen on relay circuit")?;
                // before the reservation confirms the host's address, which
                // the host relay hands out
                self.swarm.behaviour_mut().transport.relay_server =
                    host_relay::serve(local_peer_id);
                Box::new(HostRole::new(
                    self.swarm,
                    self.topic,
                    membership,
                    local_peer_id,
                    peer_info.relay,
                    reservation,
                    self.core_receiver,
                    self.message_sender,
                    self.chunk_routes,
                ))
            }
        };

        info!(%local_peer_id, "Connection setup complete");
        Ok(role)
    }

    /// Waits until every listener has reported an address (or given up).
    ///
    /// A transport only dials from a listener's port once that listener has
    /// reported its address; until then the dial goes out from a fresh
    /// ephemeral port. The relay would then observe that port, and it is the
    /// only address DCUtR can hole-punch to — nobody listens there, so every
    /// punch fails and host and client never get the direct connection that
    /// messaging needs.
    async fn await_listeners(&mut self, mut pending: HashSet<ListenerId>) {
        let reported = async {
            while !pending.is_empty() {
                match self.swarm.select_next_some().await {
                    SwarmEvent::NewListenAddr { listener_id, .. }
                    | SwarmEvent::ListenerClosed { listener_id, .. } => {
                        pending.remove(&listener_id);
                    }
                    SwarmEvent::ListenerError { listener_id, error } => {
                        warn!(?listener_id, %error, "Listener failed");
                        pending.remove(&listener_id);
                    }
                    event => debug!(?event, "Event while waiting for listeners"),
                }
            }
        };
        if tokio::time::timeout(LISTENER_PATIENCE, reported)
            .await
            .is_err()
        {
            warn!("Not every listener reported an address; dialing anyway");
        }
    }

    async fn connect(&mut self) -> Result<PeerInfo> {
        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Identify(identify::Event::Received {
                        peer_id,
                        info: identify::Info { observed_addr, .. },
                        ..
                    }),
                )) => {
                    debug!(%observed_addr, %peer_id, "Relay identified");
                    self.swarm
                        .auth()
                        .initiate(peer_id, self.room.clone(), self.password.clone());
                }
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Auth(AuthEvent::Complete { relay, host }),
                )) => return Ok(PeerInfo { relay, host }),
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Auth(AuthEvent::Denied),
                )) => return Err(AuthDenied.into()),
                SwarmEvent::Behaviour(BehaviourEvent::Transport(
                    TransportBehaviourEvent::Auth(AuthEvent::Failed(err)),
                )) => return Err(err),
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    bail!("Connection error during setup: {error}")
                }
                event => debug!(?event, "Event during connection setup"),
            }
        }
    }
}
