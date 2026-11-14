//! The messages themselves: what each one carries, and how those that
//! travel between peers look on the wire.

use std::collections::BTreeSet;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::playlist::{Playlist, Video};
use crate::room::RoomName;
use crate::user::UserStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedMsg {
    pub is_host: bool,
}

/// A peer reachable over a direct connection, so room messages can flow.
/// The host greets it with the room state. This repeats for every new
/// direct connection to the same peer, as any of them can be the peer
/// starting over after a reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerConnectedMsg {
    pub peer: PeerId,
}

/// The last direct connection to a peer closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerDisconnectedMsg {
    pub peer: PeerId,
}

/// A peer's own status, as sent to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStatusMsg {
    pub peer: PeerId,
    pub status: UserStatus,
}

/// One measured round trip to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRoundTripMsg {
    pub rtt: Duration,
}

/// What the host can address to a single peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectMessage {
    Playlist(PlaylistMsg),
    Select(SelectMsg),
    Rename(ForcedRenameMsg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToPeerMsg {
    pub peer: PeerId,
    pub message: DirectMessage,
}

impl ToPeerMsg {
    pub fn new(peer: PeerId, message: DirectMessage) -> Self {
        Self { peer, message }
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStatusMsg {
    pub video: Option<Video>,
    #[serde(with = "serde_millis")]
    pub position: Option<Duration>,
    pub speed: f64,
    pub paused: bool,
    pub file_loaded: bool,
}

impl PartialEq for VideoStatusMsg {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(other.speed);
        speed_self.eq(&speed_other)
            && self.video.eq(&other.video)
            && self.position.eq(&other.position)
            && self.paused.eq(&other.paused)
    }
}

impl Eq for VideoStatusMsg {}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusListMsg {
    pub room_name: RoomName,
    pub users: BTreeSet<UserStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartMsg {
    pub actor: ArcStr,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PauseMsg {
    pub actor: ArcStr,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSpeedMsg {
    pub actor: ArcStr,
    pub speed: f64,
}

impl PartialEq for PlaybackSpeedMsg {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(other.speed);
        speed_self.eq(&speed_other) && self.actor.eq(&other.actor)
    }
}

impl Eq for PlaybackSpeedMsg {}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SeekMsg {
    pub actor: ArcStr,
    pub video: Video,
    #[serde(with = "serde_millis")]
    pub position: Duration,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectMsg {
    pub actor: ArcStr,
    #[serde(with = "serde_millis")]
    pub position: Duration,
    pub video: Option<Video>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageMsg {
    pub actor: ArcStr,
    pub message: String,
}

/// A line for the chat that no user wrote: what became of the connection or
/// of a file share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeMsg {
    pub message: String,
}

impl NoticeMsg {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMsg {
    pub actor: ArcStr,
    #[serde(flatten)]
    pub playlist: Playlist,
}

/// A user's own status, as the user announces it.
pub type UserStatusMsg = UserStatus;

/// The host renames a peer whose name another user of the room goes by.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForcedRenameMsg {
    pub name: ArcStr,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileRequestMsg {
    /// Pairs the provider's answer with this request.
    pub uuid: uuid::Uuid,
    pub actor: ArcStr,
    pub video: Video,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileResponseMsg {
    pub uuid: uuid::Uuid,
    pub actor: ArcStr,
    pub video: Video,
    pub size: u64,
}

/// The provider cannot answer the request `uuid` with a file. The requester
/// is told the `reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDeclineMsg {
    pub uuid: uuid::Uuid,
    pub reason: String,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoShareMsg {
    pub video: Option<Video>,
}

impl VideoShareMsg {
    pub fn new(video: Video) -> Self {
        Self { video: Some(video) }
    }
}

/// The host handed the room over to the user `new_host`. If that is this
/// peer, it `becomes_host` by connecting again: the relay then names it
/// the room's host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHandoverMsg {
    pub new_host: ArcStr,
    pub becomes_host: bool,
}

/// The host hands the room over to `peer`, which goes by `new_host`. The
/// communicator asks the relay to transfer the room and announces the new
/// host to the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHandoverRequest {
    pub peer: PeerId,
    pub new_host: ArcStr,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoProviderStoppedMsg;
