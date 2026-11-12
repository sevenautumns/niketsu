//! Broadcasts of one peer do not arrive in the order they were sent: two
//! peers can share several connections, and each broadcast takes whichever
//! is free. Most broadcasts replace a piece of room state, so an older one
//! arriving late would put stale state back.

use std::collections::HashMap;

use libp2p::PeerId;

use crate::messages::NiketsuMessage;

/// The piece of room state a broadcast replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum State {
    VideoStatus,
    Users,
    Playing,
    Speed,
    Position,
    Selection,
    Playlist,
}

impl State {
    /// `None` for broadcasts that stand on their own, like a chat line.
    fn of(msg: &NiketsuMessage) -> Option<Self> {
        use NiketsuMessage::*;
        match msg {
            VideoStatus(_) => Some(Self::VideoStatus),
            StatusList(_) => Some(Self::Users),
            Pause(_) | Start(_) => Some(Self::Playing),
            PlaybackSpeed(_) => Some(Self::Speed),
            Seek(_) => Some(Self::Position),
            Select(_) => Some(Self::Selection),
            Playlist(_) => Some(Self::Playlist),
            UserMessage(_) | Status(_) | HostHandover(_) => None,
        }
    }
}

/// The sequence number of the newest broadcast seen per sender and state.
/// Gossipsub numbers the broadcasts of a peer in the order it publishes.
#[derive(Debug, Default)]
pub(super) struct Newest(HashMap<(PeerId, State), u64>);

impl Newest {
    /// Whether `msg` is older than a broadcast of `source` that already
    /// replaced the same state. Takes note of `msg` otherwise.
    pub(super) fn is_stale(&mut self, source: PeerId, sequence: u64, msg: &NiketsuMessage) -> bool {
        let Some(state) = State::of(msg) else {
            return false;
        };
        let newest = self.0.entry((source, state)).or_insert(sequence);
        if sequence < *newest {
            return true;
        }
        *newest = sequence;
        false
    }
}

#[cfg(test)]
mod tests {
    use libp2p::identity::Keypair;
    use niketsu_core::communicator::{PauseMsg, StartMsg, UserMessageMsg, UserStatusListMsg};

    use super::*;

    fn peer() -> PeerId {
        Keypair::generate_ed25519().public().to_peer_id()
    }

    fn users() -> NiketsuMessage {
        UserStatusListMsg {
            room_name: "room".into(),
            users: Default::default(),
        }
        .into()
    }

    fn chat() -> NiketsuMessage {
        UserMessageMsg {
            actor: "alice".into(),
            message: "hi".into(),
        }
        .into()
    }

    #[test]
    fn a_late_older_broadcast_of_the_same_state_is_stale() {
        let host = peer();
        let mut newest = Newest::default();
        assert!(!newest.is_stale(host, 7, &users()));
        assert!(newest.is_stale(host, 6, &users()));
        assert!(!newest.is_stale(host, 8, &users()));
    }

    #[test]
    fn pause_and_start_replace_each_other() {
        let start = StartMsg {
            actor: "alice".into(),
        };
        let pause = PauseMsg {
            actor: "alice".into(),
        };
        let peer = peer();
        let mut newest = Newest::default();
        assert!(!newest.is_stale(peer, 2, &start.into()));
        assert!(newest.is_stale(peer, 1, &pause.into()));
    }

    #[test]
    fn other_senders_and_other_states_are_unaffected() {
        let host = peer();
        let mut newest = Newest::default();
        assert!(!newest.is_stale(host, 7, &users()));
        assert!(!newest.is_stale(peer(), 1, &users()));
        assert!(!newest.is_stale(host, 1, &chat()));
        assert!(!newest.is_stale(host, 0, &chat()));
    }
}
