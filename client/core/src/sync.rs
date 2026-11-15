//! Judging the room's state as it reaches a follower: when this core's own
//! intents are still on their way, and how far the host's clock lags behind
//! by the time it arrives.
//!
//! The host's messages reach a member in the order the host sent them, so
//! every state that arrives is the newest the host has sent this client.

use std::collections::VecDeque;
use std::time::Duration;

use crate::communicator::RoomStateMsg;
use crate::playlist::Playlist;

/// Number of round-trip samples the delay estimate is taken over. The
/// communicator pings the host once a second, so this covers the last ten.
const DELAY_WINDOW: usize = 10;

/// How many states a client ignores while an intent of its own is pending.
/// The host publishes with every heartbeat, so this bounds how long an
/// intent the host never handled keeps the client apart from the room.
pub const PENDING_STATES: u8 = 4;

/// Round-trip samples to the host, of which the estimate is the *minimum*:
/// congestion and scheduling only ever add to a round-trip time, so the
/// smallest recent sample is the one least polluted by them. The estimate
/// shifts the host position we reconcile against, so a single spike on a
/// flaky link must not move it — which is exactly what an average would do.
#[derive(Debug, Default, Clone)]
pub struct HostDelay {
    samples: VecDeque<Duration>,
}

impl HostDelay {
    pub fn push(&mut self, sample: Duration) {
        if self.samples.len() == DELAY_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Zero until the first sample arrives, which leaves the host's reported
    /// position uncorrected rather than guessing at a delay.
    pub fn round_trip(&self) -> Duration {
        self.samples.iter().copied().min().unwrap_or_default()
    }

    /// The one-way share of the round trip: how old a host position is by
    /// the time it arrives here.
    pub fn one_way(&self) -> Duration {
        self.round_trip().div_f64(2.0)
    }
}

/// Intents this client sent that the host has not answered yet. A state the
/// host published before handling them does not show what this user did.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// The first version that can include every one of them: the host
    /// bumps the version once for each intent it handles.
    until: u64,
    /// States still to ignore before giving up on them.
    states: u8,
}

/// What a follower needs to judge the host's room states. Belongs to one
/// connection: a new host counts its versions anew.
#[derive(Debug, Default, Clone)]
pub struct ClientSync {
    pub delay: HostDelay,
    /// The version of the state taken last.
    taken: Option<u64>,
    pending: Option<Pending>,
    /// The room's playlist as the host sent it last. A state carries it
    /// only until this client has it, also when the state itself is
    /// ignored, so it is kept here apart from the state taken.
    pub playlist: Playlist,
}

impl ClientSync {
    /// Notes an intent sent to the host while the state taken here is the
    /// latest known.
    pub fn intent_sent(&mut self) {
        let taken = self.taken.unwrap_or(0);
        let sent_before = self.pending.map_or(taken, |p| p.until.max(taken));
        self.pending = Some(Pending {
            until: sent_before + 1,
            states: PENDING_STATES,
        });
    }

    /// Whether `state` is to be taken. A state that predates the pending
    /// intents is not, until they expire.
    pub fn admit(&mut self, state: &RoomStateMsg) -> bool {
        if let Some(pending) = &mut self.pending
            && state.version < pending.until
            && pending.states > 0
        {
            pending.states -= 1;
            return false;
        }
        self.pending = None;
        self.taken = Some(state.version);
        true
    }

    /// The host's reported position, shifted by how long it was underway.
    /// A paused host does not move, so there is nothing to shift.
    pub fn host_position(&self, reported: Duration, paused: bool) -> Duration {
        if paused {
            reported
        } else {
            reported + self.delay.one_way()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(version: u64) -> RoomStateMsg {
        RoomStateMsg {
            version,
            ..Default::default()
        }
    }

    #[test]
    fn no_samples_means_no_correction() {
        assert_eq!(HostDelay::default().round_trip(), Duration::ZERO);
        let sync = ClientSync::default();
        assert_eq!(
            sync.host_position(Duration::from_secs(10), false),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn a_single_spike_does_not_move_the_delay() {
        let mut delay = HostDelay::default();
        for _ in 0..5 {
            delay.push(Duration::from_millis(40));
        }
        delay.push(Duration::from_secs(4));

        assert_eq!(
            delay.round_trip(),
            Duration::from_millis(40),
            "one congested sample must not shift the host position"
        );
    }

    #[test]
    fn a_sustained_change_is_followed_once_the_window_turns_over() {
        let mut delay = HostDelay::default();
        delay.push(Duration::from_millis(40));
        for _ in 0..DELAY_WINDOW {
            delay.push(Duration::from_millis(200));
        }

        assert_eq!(
            delay.round_trip(),
            Duration::from_millis(200),
            "a link that is genuinely slower must be picked up"
        );
    }

    #[test]
    fn half_the_round_trip_is_added_only_while_playing() {
        let mut sync = ClientSync::default();
        sync.delay.push(Duration::from_millis(100));

        assert_eq!(
            sync.host_position(Duration::from_secs(10), false),
            Duration::from_millis(10_050)
        );
        assert_eq!(
            sync.host_position(Duration::from_secs(10), true),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn every_state_is_taken_while_no_intent_is_pending() {
        let mut sync = ClientSync::default();
        assert!(sync.admit(&state(3)));
        assert!(sync.admit(&state(3)), "a heartbeat");
        assert!(sync.admit(&state(4)));
    }

    #[test]
    fn a_state_from_before_the_hosts_answer_is_ignored() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3));
        sync.intent_sent();

        assert!(!sync.admit(&state(3)));
        assert!(sync.admit(&state(4)), "the host answered");
        assert!(sync.admit(&state(4)));
    }

    #[test]
    fn every_pending_intent_has_to_be_answered() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3));
        sync.intent_sent();
        sync.intent_sent();

        assert!(!sync.admit(&state(4)), "only the first answered");
        assert!(sync.admit(&state(5)));
    }

    #[test]
    fn an_unanswered_intent_expires() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3));
        sync.intent_sent();

        for _ in 0..PENDING_STATES {
            assert!(!sync.admit(&state(3)));
        }
        assert!(sync.admit(&state(3)), "the intent was lost");
        assert!(sync.admit(&state(3)));
    }
}
