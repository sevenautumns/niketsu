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
    fn connect(&mut self, connect: EndpointInfo);
    fn send(&mut self, msg: OutgoingMessage);
    async fn receive(&mut self) -> IncomingMessage;
    fn has_endpoint(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub addr: Multiaddr,
    pub room: RoomName,
    pub password: String,
}

/// What the core asks the communicator to deliver. The communicator decides
/// only *how* a message travels for its role (gossip broadcast, or a direct
/// request to the host); *whether* and *what* to send is decided here.
///
/// Messages are grouped by the role that may send them, so each role's
/// handler matches exhaustively over its own group. Which role the core
/// holds is a runtime fact (the relay assigns it on connect, and a message
/// can still be queued while the role changes), so a message of the other
/// role's group can still reach a handler; it is dropped there.
#[derive(Clone, Debug, PartialEq)]
pub enum OutgoingMessage {
    Room(RoomMessage),
    Host(HostInstruction),
    /// What only a client sends: a request to the host.
    Client(IntentMsg),
}

/// Sent the same way whichever role this core holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomMessage {
    UserMessage(UserMessageMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    FileDecline(FileDeclineMsg),
    VideoShareChange(VideoShareMsg),
}

/// What only the host sends.
#[derive(Clone, Debug, PartialEq)]
pub enum HostInstruction {
    /// Everything the room agrees on, sent to the room.
    RoomState(RoomStateMsg),
    /// What the host did on a user's behalf, sent to the room's chat.
    Action(ActionMsg),
    /// A message for exactly one peer rather than the room.
    ToPeer(ToPeerMsg),
}

impl From<RoomMessage> for OutgoingMessage {
    fn from(value: RoomMessage) -> Self {
        Self::Room(value)
    }
}

impl From<HostInstruction> for OutgoingMessage {
    fn from(value: HostInstruction) -> Self {
        Self::Host(value)
    }
}

impl From<IntentMsg> for OutgoingMessage {
    fn from(value: IntentMsg) -> Self {
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
    UserMessageMsg => Room(RoomMessage::UserMessage),
    FileRequestMsg => Room(RoomMessage::FileRequest),
    FileResponseMsg => Room(RoomMessage::FileResponse),
    FileDeclineMsg => Room(RoomMessage::FileDecline),
    VideoShareMsg => Room(RoomMessage::VideoShareChange),
    RoomStateMsg => Host(HostInstruction::RoomState),
    ActionMsg => Host(HostInstruction::Action),
    ToPeerMsg => Host(HostInstruction::ToPeer),
}

/// What the communicator hands the core. Each message is an event of the
/// core's loop.
#[enum_dispatch(EventHandler)]
#[derive(Clone, Debug)]
pub enum IncomingMessage {
    RoomState(RoomStateMsg),
    Action(ActionMsg),
    Connected(ConnectedMsg),
    UserMessage(UserMessageMsg),
    Notice(NoticeMsg),
    ForcedRename(ForcedRenameMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    VideoProviderStopped(VideoProviderStoppedMsg),
    PeerConnected(PeerConnectedMsg),
    PeerDisconnected(PeerDisconnectedMsg),
    PeerIntent(PeerIntentMsg),
    HostRoundTrip(HostRoundTripMsg),
}

impl From<DirectMessage> for IncomingMessage {
    fn from(value: DirectMessage) -> Self {
        match value {
            DirectMessage::RoomState(msg) => msg.into(),
            DirectMessage::Rename(msg) => msg.into(),
        }
    }
}
