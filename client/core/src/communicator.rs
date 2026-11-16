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

/// What the core asks the communicator to deliver. Whether and what to
/// send is decided here; the communicator decides only how a message
/// travels.
///
/// Which role the core holds is a runtime fact (the relay assigns it on
/// connect, and a message can still be queued while the role changes), so
/// a message can reach a communicator that has the other role. It is
/// dropped there.
#[derive(Clone, Debug, PartialEq)]
pub enum OutgoingMessage {
    /// From the host: everything the room agrees on, to every member, with
    /// the playlist, which a member gets once. Each member gets
    /// the host's messages in the order the host sent them, except that a
    /// room state still waiting for its turn gives way to a newer one.
    RoomState(RoomStateMsg),
    /// From the host: a line of the room's chat, to every member but the
    /// author of a chat line.
    Line(LineMsg),
    /// From a client: a request to the host.
    Intent(Intent),
}

impl From<RoomStateMsg> for OutgoingMessage {
    fn from(value: RoomStateMsg) -> Self {
        Self::RoomState(value)
    }
}

impl From<LineMsg> for OutgoingMessage {
    fn from(value: LineMsg) -> Self {
        Self::Line(value)
    }
}

impl From<Intent> for OutgoingMessage {
    fn from(value: Intent) -> Self {
        Self::Intent(value)
    }
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
