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
    /// This peer's own id, under which the room's users reach it.
    pub peer: PeerId,
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
/// The host bumps `version` with every change of the room and every intent
/// it handles; a heartbeat repeats the version and only refreshes
/// `position`.
///
/// The playlist is too large to go with every heartbeat. The host's core
/// attaches it to every state, and the host's communicator leaves it out
/// for a member that has acknowledged a state carrying the playlist of
/// `playlist_version`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStateMsg {
    pub version: u64,
    /// Bumped whenever the position jumps, by a seek or a selection. A
    /// follower sets such a position exactly; any other only has to be
    /// within the tolerance.
    pub jumps: u64,
    /// The version of the change that set the room's playlist. It names
    /// that playlist and is only ever compared for equality.
    pub playlist_version: u64,
    pub playlist: Option<Playlist>,
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

/// The chat line for what the host did on a user's behalf. The room state
/// carries the outcome, so this carries no more than the line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionMsg {
    pub actor: ArcStr,
    pub message: String,
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

/// The host renames a peer whose name another user of the room goes by.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForcedRenameMsg {
    pub name: ArcStr,
}

/// The local video server's chunks are fetched from `provider`, a user the
/// room state shows offering the video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamMsg {
    pub provider: PeerId,
}

/// The stream the local video server plays has ended. The communicator
/// tells the core when the provider's side ended it; the core tells the
/// communicator when it ended the stream itself.
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamEndedMsg;
