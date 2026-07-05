use std::task::{Context, Poll};

use either::Either;
use libp2p::core::transport::PortUse;
use libp2p::core::{Endpoint, Multiaddr};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent,
    ToSwarm, dummy,
};
use libp2p::{PeerId, relay};

type Inner = relay::Behaviour;

/// Relay server that only serves connections established after `enable()`.
///
/// Gating works by handing out a `dummy::ConnectionHandler` (which does not
/// speak the relay protocols at all) instead of the real relay handler.
/// `enable()` is called once this peer becomes the room host, before any
/// client dials it, so all client connections get real handlers while
/// non-host peers never serve relay traffic.
///
/// Unlike [`super::direct_only::DirectOnly`], no [`FromSwarm`] events are
/// withheld from `inner` for dummy-handler connections: [`relay::Behaviour`]
/// internally hands out dummy handlers itself (for relayed connections), so it
/// is built to tolerate swarm events for connections it never serves — its
/// `ConnectionClosed` handling is guarded map cleanup with no paired-event
/// assertions (as of libp2p-relay 0.21.1; re-check when upgrading).
pub(crate) struct GatedRelayBehaviour {
    inner: Inner,
    enabled: bool,
}

impl GatedRelayBehaviour {
    pub(crate) fn new(local_peer_id: PeerId, config: relay::Config) -> Self {
        Self {
            inner: relay::Behaviour::new(local_peer_id, config),
            enabled: false,
        }
    }

    pub(crate) fn enable(&mut self) {
        self.enabled = true;
    }
}

impl NetworkBehaviour for GatedRelayBehaviour {
    type ConnectionHandler =
        Either<<Inner as NetworkBehaviour>::ConnectionHandler, dummy::ConnectionHandler>;
    type ToSwarm = <Inner as NetworkBehaviour>::ToSwarm;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        if !self.enabled {
            return Ok(());
        }
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        if !self.enabled {
            return Ok(Either::Right(dummy::ConnectionHandler));
        }
        self.inner
            .handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
            .map(Either::Left)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        if !self.enabled {
            return Ok(Either::Right(dummy::ConnectionHandler));
        }
        self.inner
            .handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
                port_use,
            )
            .map(Either::Left)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            Either::Left(event) => {
                self.inner
                    .on_connection_handler_event(peer_id, connection_id, event)
            }
            Either::Right(infallible) => match infallible {},
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.inner.poll(cx).map(|event| event.map_in(Either::Left))
    }
}
