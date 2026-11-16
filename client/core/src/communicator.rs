//! The core's side of the communicator: the trait a communicator
//! implements, and the messages that pass between the two. What a message
//! carries is in [`messages`]; what the core does on receiving one and how
//! it reads in the chat are in the private modules beside it.

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use multiaddr::Multiaddr;
pub use multiaddr::PeerId;

use crate::room::RoomName;

mod chat;
mod handlers;
mod messages;

pub use messages::*;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommunicatorTrait: std::fmt::Debug + Send {
    /// This peer's own id, under which the room's users know it. It is the
    /// same in and between every session.
    fn peer(&self) -> PeerId;
    fn connect(&mut self, connect: EndpointInfo);
    fn send(&mut self, msg: OutgoingMessage);
    async fn receive(&mut self) -> IncomingMessage;
    fn has_endpoint(&self) -> bool;
}

#[cfg(test)]
impl MockCommunicatorTrait {
    /// A communicator that has a peer id and expects nothing else.
    pub fn with_peer() -> Self {
        let mut communicator = Self::default();
        let peer = PeerId::from_bytes(&[0x00, 4, 1, 1, 1, 1]).expect("an identity multihash");
        communicator.expect_peer().return_const(peer);
        communicator
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub addr: Multiaddr,
    pub room: RoomName,
    pub password: String,
}

/// What the core asks the communicator to deliver. The communicator decides
/// only *how* a message travels for its role (from the host to each member,
/// or from a client to the host); *whether* and *what* to send is decided
/// here.
///
/// Messages are grouped by the role that may send them, so each role's
/// handler matches exhaustively over its own group. Which role the core
/// holds is a runtime fact (the relay assigns it on connect, and a message
/// can still be queued while the role changes), so a message of the other
/// role's group can still reach a handler; it is dropped there.
#[derive(Clone, Debug, PartialEq)]
pub enum OutgoingMessage {
    Host(HostInstruction),
    Client(ClientRequest),
}

/// What only the host sends. Each member gets the host's messages in the
/// order the host sent them, except that a room state still waiting for
/// its turn gives way to a newer one.
#[derive(Clone, Debug, PartialEq)]
pub enum HostInstruction {
    /// Everything the room agrees on, sent to every member with the
    /// playlist, which a member gets until it has it.
    RoomState(RoomStateMsg),
    /// A line of the room's chat, sent to every member but the author of a
    /// chat line.
    Line(LineMsg),
}

/// What only a client sends: a request to the host.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientRequest {
    Intent(Intent),
}

impl From<HostInstruction> for OutgoingMessage {
    fn from(value: HostInstruction) -> Self {
        Self::Host(value)
    }
}

impl From<ClientRequest> for OutgoingMessage {
    fn from(value: ClientRequest) -> Self {
        Self::Client(value)
    }
}

/// `From<$msg> for OutgoingMessage` for messages whose sending role follows
/// from their type.
macro_rules! outgoing_from {
    ($($msg:ty => $outer:ident($group:ident::$variant:ident)),* $(,)?) => {
        $(impl From<$msg> for OutgoingMessage {
            fn from(value: $msg) -> Self {
                Self::$outer($group::$variant(value))
            }
        })*
    };
}

outgoing_from! {
    RoomStateMsg => Host(HostInstruction::RoomState),
    LineMsg => Host(HostInstruction::Line),
    Intent => Client(ClientRequest::Intent),
}

/// What the communicator hands the core. Each message is an event of the
/// core's loop.
#[enum_dispatch(EventHandler)]
#[derive(Clone, Debug)]
pub enum IncomingMessage {
    RoomState(RoomStateMsg),
    Line(LineMsg),
    Connected(ConnectedMsg),
    Disconnected(DisconnectedMsg),
    PeerDisconnected(PeerDisconnectedMsg),
    PeerIntent(PeerIntentMsg),
    HostRoundTrip(HostRoundTripMsg),
}
