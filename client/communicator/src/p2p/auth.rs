use std::task::{Context, Poll};

use anyhow::anyhow;
use libp2p::core::Endpoint;
use libp2p::core::transport::PortUse;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent,
    ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use niketsu_core::room::RoomName;
use niketsu_protocol::{AUTH_PROTOCOL, AuthStatus, InitRequest, InitResponse};
use sha256::digest;

use crate::timing::AUTH_TIMEOUT;

type Inner = request_response::cbor::Behaviour<InitRequest, InitResponse>;

/// The relay only ever sees a digest of the room's password.
fn request(room: RoomName, password: String, transfer_to: Option<PeerId>) -> InitRequest {
    InitRequest {
        room: room.to_string(),
        password: digest(password),
        transfer_to,
    }
}

#[derive(Debug)]
pub(crate) enum AuthEvent {
    Complete { relay: PeerId, host: Option<PeerId> },
    Failed(anyhow::Error),
}

pub(crate) struct AuthBehaviour {
    inner: Inner,
    relay: Option<PeerId>,
}

impl AuthBehaviour {
    pub(crate) fn new() -> Self {
        Self {
            inner: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new(AUTH_PROTOCOL),
                    // Only the relay serves auth requests; clients never
                    // accept inbound ones.
                    ProtocolSupport::Outbound,
                )],
                request_response::Config::default().with_request_timeout(AUTH_TIMEOUT),
            ),
            relay: None,
        }
    }

    pub(crate) fn initiate(&mut self, relay: PeerId, room: RoomName, password: String) {
        self.inner
            .send_request(&relay, request(room, password, None));
        self.relay = Some(relay);
    }

    pub(crate) fn transfer(
        &mut self,
        relay: PeerId,
        room: RoomName,
        password: String,
        new_host: PeerId,
    ) {
        self.inner
            .send_request(&relay, request(room, password, Some(new_host)));
        self.relay = Some(relay);
    }
}

impl NetworkBehaviour for AuthBehaviour {
    type ConnectionHandler = <Inner as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = AuthEvent;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
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
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
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
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )
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
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        loop {
            match self.inner.poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                    let auth_event = match event {
                        request_response::Event::Message {
                            message: request_response::Message::Response { response, .. },
                            ..
                        } => {
                            let Some(relay) = self.relay else { continue };
                            match response.status {
                                AuthStatus::Ok => AuthEvent::Complete {
                                    relay,
                                    host: response.peer_id,
                                },
                                AuthStatus::Err => {
                                    AuthEvent::Failed(anyhow!("Authentication failed"))
                                }
                            }
                        }
                        request_response::Event::OutboundFailure { error, .. } => {
                            AuthEvent::Failed(anyhow!("Auth request failed: {error}"))
                        }
                        _ => continue,
                    };
                    return Poll::Ready(ToSwarm::GenerateEvent(auth_event));
                }
                Poll::Ready(ev) => return Poll::Ready(ev.map_out(|_| unreachable!())),
            }
        }
    }
}
