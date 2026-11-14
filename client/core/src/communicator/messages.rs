//! The messages themselves: what each one carries, and how those that
//! travel between peers look on the wire.

use std::collections::BTreeSet;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
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

/// What a peer asked the host for.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerIntentMsg {
    pub peer: PeerId,
    pub intent: IntentMsg,
}

/// One measured round trip to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRoundTripMsg {
    pub rtt: Duration,
}

/// What the host can address to a single peer.
#[derive(Debug, Clone, PartialEq)]
pub enum DirectMessage {
    /// The greeting of a peer that joined.
    RoomState(RoomStateMsg),
    Rename(ForcedRenameMsg),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToPeerMsg {
    pub peer: PeerId,
    pub message: DirectMessage,
}

impl ToPeerMsg {
    pub fn new(peer: PeerId, message: DirectMessage) -> Self {
        Self { peer, message }
    }
}

/// Everything the room agrees on, as the host publishes it.
///
/// States are ordered by `(version, beat)`: the host bumps `version` with
/// every change of the room and every intent it handles, and `beat` with
/// every heartbeat, which only refreshes `position`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStateMsg {
    pub version: u64,
    pub beat: u64,
    /// Bumped whenever the position jumps, by a seek or a selection. A
    /// follower sets such a position exactly; any other only has to be
    /// within the tolerance.
    pub jumps: u64,
    pub playlist: Playlist,
    pub video: Option<Video>,
    #[serde(with = "serde_millis")]
    pub position: Duration,
    pub paused: bool,
    pub speed: f64,
    /// Whether the host's player has the video loaded. Until then its
    /// position and play state are not the room's.
    pub file_loaded: bool,
    pub users: UserStatusListMsg,
}

impl RoomStateMsg {
    pub fn order(&self) -> (u64, u64) {
        (self.version, self.beat)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusListMsg {
    pub room_name: RoomName,
    pub users: BTreeSet<UserStatus>,
}

/// What a user wants of the room. The host decides and publishes the
/// outcome in its room state.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    Pause,
    Start,
    Speed(f64),
    Seek {
        video: Video,
        #[serde(with = "serde_millis")]
        position: Duration,
    },
    Select {
        video: Option<Video>,
        #[serde(with = "serde_millis")]
        position: Duration,
    },
    Playlist(Playlist),
    /// The user's own name and readiness.
    Status(UserStatus),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentMsg {
    pub actor: ArcStr,
    pub intent: Intent,
}

/// What the host did on a user's behalf, for the chat only. The room state
/// carries the outcome.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ActionMsg(pub IntentMsg);

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

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoProviderStoppedMsg;
