//! The messages themselves: what each one carries, and how those that
//! travel between peers look on the wire.

use std::collections::BTreeMap;
use std::time::Duration;

use arcstr::ArcStr;
use multiaddr::PeerId;
use serde::{Deserialize, Serialize};

use crate::playlist::{Playlist, Video};
use crate::user::User;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectedMsg {
    pub is_host: bool,
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
    pub intent: Intent,
}

/// One measured round trip to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRoundTripMsg {
    pub rtt: Duration,
}

/// Everything the room agrees on, as the host publishes it.
///
/// `position` is where the room is at the moment the state is sent. Each
/// user's `handled` tells that user whether the state is the outcome of
/// what it asked.
///
/// The playlist is too large to go with every heartbeat. The host's core
/// attaches it to every state, and the host's communicator leaves it out
/// for a member it has sent that playlist already.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomStateMsg {
    /// Bumped whenever the position jumps, by a seek or a selection. A
    /// follower sets such a position exactly; with any other its player is
    /// judged by the offset it keeps.
    pub jumps: u64,
    pub playlist: Option<Playlist>,
    pub video: Option<Video>,
    #[serde(with = "serde_millis")]
    pub position: Duration,
    pub paused: bool,
    pub speed: f64,
    /// Every user of the room, the host's own included, by peer id.
    pub users: BTreeMap<PeerId, User>,
}

/// What a user wants of the room. The host decides and publishes the
/// outcome in its room state. Who asks is the peer the intent came from.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    /// The user paused, and is not ready until they start again.
    Pause,
    /// The user's player waits for its cache to fill, and the room with
    /// it. The user stays as ready as before.
    CachePause,
    /// The user started playback, and is ready.
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
    /// The user's player reached the end of this video. The room moves on
    /// to the video after it, unless it has moved on already.
    Ended(Video),
    Playlist(Playlist),
    /// The user's own name and readiness, and what its player has loaded
    /// and offers. The user is in the room from its first status on.
    Status(User),
    /// A line the user wrote in the chat.
    Chat(String),
}

/// A line of the room's chat, which the host stamps with the name of the
/// user it is about.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineMsg {
    pub actor: ArcStr,
    pub message: String,
    pub kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    /// What the host did on the actor's behalf. Everyone shows it when it
    /// arrives, the actor included.
    Action,
    /// What the actor wrote. Its author's chat shows it from the moment it
    /// was written, so the line goes to everyone else.
    Chat { author: PeerId },
}

impl LineMsg {
    /// The peer whose chat shows this line already.
    pub fn author(&self) -> Option<PeerId> {
        match self.kind {
            LineKind::Chat { author } => Some(author),
            LineKind::Action => None,
        }
    }
}

/// This core is in no room: its session ended, or a connection attempt
/// failed, for `reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectedMsg {
    pub reason: String,
}
