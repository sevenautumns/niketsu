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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutgoingMessage {
    Room(RoomMessage),
    Host(HostInstruction),
    Client(ClientRequest),
}

/// Sent the same way whichever role this core holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomMessage {
    Start(StartMsg),
    Pause(PauseMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    FileDecline(FileDeclineMsg),
    VideoShareChange(VideoShareMsg),
}

/// What only the host sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostInstruction {
    /// The host's clock, the room's reference.
    VideoStatus(VideoStatusMsg),
    /// The room's playlist, sent to the room.
    Playlist(PlaylistMsg),
    /// The host's view of who is in the room, sent to the room.
    UserStatusList(UserStatusListMsg),
    /// A message for exactly one peer rather than the room.
    ToPeer(ToPeerMsg),
}

/// What only a client sends, each a request to the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientRequest {
    /// An edit of the room's playlist, for the host to adopt.
    Playlist(PlaylistMsg),
    /// The client's own status.
    UserStatus(UserStatusMsg),
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

impl From<ClientRequest> for OutgoingMessage {
    fn from(value: ClientRequest) -> Self {
        Self::Client(value)
    }
}

/// `From<$msg> for OutgoingMessage` for messages whose sending role follows
/// from their type. A playlist is sent by both roles with different meaning,
/// so it is wrapped explicitly.
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
    StartMsg => Room(RoomMessage::Start),
    PauseMsg => Room(RoomMessage::Pause),
    PlaybackSpeedMsg => Room(RoomMessage::PlaybackSpeed),
    SeekMsg => Room(RoomMessage::Seek),
    SelectMsg => Room(RoomMessage::Select),
    UserMessageMsg => Room(RoomMessage::UserMessage),
    FileRequestMsg => Room(RoomMessage::FileRequest),
    FileResponseMsg => Room(RoomMessage::FileResponse),
    FileDeclineMsg => Room(RoomMessage::FileDecline),
    VideoShareMsg => Room(RoomMessage::VideoShareChange),
    VideoStatusMsg => Host(HostInstruction::VideoStatus),
    UserStatusListMsg => Host(HostInstruction::UserStatusList),
    ToPeerMsg => Host(HostInstruction::ToPeer),
    UserStatusMsg => Client(ClientRequest::UserStatus),
}

/// What the communicator hands the core. Each message is an event of the
/// core's loop.
#[enum_dispatch(EventHandler)]
#[derive(Clone, Debug)]
pub enum IncomingMessage {
    VideoStatus(VideoStatusMsg),
    Connected(ConnectedMsg),
    UserStatusList(UserStatusListMsg),
    Start(StartMsg),
    Pause(PauseMsg),
    PlaybackSpeed(PlaybackSpeedMsg),
    Seek(SeekMsg),
    Select(SelectMsg),
    UserMessage(UserMessageMsg),
    Notice(NoticeMsg),
    Playlist(PlaylistMsg),
    ForcedRename(ForcedRenameMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    VideoProviderStopped(VideoProviderStoppedMsg),
    PeerConnected(PeerConnectedMsg),
    PeerDisconnected(PeerDisconnectedMsg),
    PeerStatus(PeerStatusMsg),
    HostRoundTrip(HostRoundTripMsg),
}

impl From<DirectMessage> for IncomingMessage {
    fn from(value: DirectMessage) -> Self {
        match value {
            DirectMessage::Playlist(msg) => msg.into(),
            DirectMessage::Select(msg) => msg.into(),
            DirectMessage::Rename(msg) => msg.into(),
        }
    }
}
