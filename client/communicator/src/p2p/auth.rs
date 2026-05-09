use std::collections::VecDeque;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::anyhow;
use libp2p::core::{Endpoint, transport::PortUse};
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent,
    ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use niketsu_core::room::RoomName;
use serde::{Deserialize, Serialize};
use sha256::digest;

use super::StatusResponse;

type Inner = request_response::cbor::Behaviour<InitRequest, InitResponse>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitRequest {
    room: RoomName,
    password: String,
}

impl InitRequest {
    fn new(room: RoomName, password: String) -> Self {
        Self {
            room,
            password: digest(password),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: StatusResponse,
    peer_id: Option<PeerId>,
}

#[derive(Debug)]
pub(crate) enum AuthEvent {
    Complete { relay: PeerId, host: Option<PeerId> },
    Failed(anyhow::Error),
}

pub(crate) struct AuthBehaviour {
    inner: Inner,
    relay: Option<PeerId>,
    events: VecDeque<AuthEvent>,
}

impl AuthBehaviour {
    pub(crate) fn new() -> Self {
        Self {
            inner: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/authorisation/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default()
                    .with_request_timeout(Duration::from_secs(10)),
            ),
            relay: None,
            events: VecDeque::new(),
        }
    }

    pub(crate) fn initiate(&mut self, relay: PeerId, room: RoomName, password: String) {
        self.inner.send_request(&relay, InitRequest::new(room, password));
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
        self.inner
            .handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
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
        if let Some(ev) = self.events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(ev));
        }

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
                                StatusResponse::Ok => AuthEvent::Complete {
                                    relay,
                                    host: response.peer_id,
                                },
                                StatusResponse::Err => {
                                    AuthEvent::Failed(anyhow!("Authentication failed"))
                                }
                                _ => AuthEvent::Failed(anyhow!("Unexpected response from relay")),
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
