//! Judging the room's state as it reaches a follower: when this core's own
//! intents are still on their way, and how far the host's clock lags behind
//! by the time it arrives.
//!
//! The host's messages reach a member in the order the host sent them, so
//! every state that arrives is the newest the host has sent this client.

use std::collections::VecDeque;
use std::time::Duration;

use crate::playlist::Playlist;
use crate::user::User;

/// Number of round-trip samples the delay estimate is taken over. The
/// communicator pings the host once a second, so this covers the last ten.
const DELAY_WINDOW: usize = 10;

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

/// What a follower needs to judge the host's room states. Belongs to one
/// session: the host counts what it handled of a member from the moment
/// the member joined.
#[derive(Debug, Default, Clone)]
pub struct ClientSync {
    pub delay: HostDelay,
    /// How many intents this client has sent the host in this session.
    pub sent: u64,
    /// The room's playlist as the host sent it last. A state carries it
    /// only until this client has it, also when the state itself is
    /// ignored, so it is kept here apart from the state taken.
    pub playlist: Playlist,
}

impl ClientSync {
    /// Whether a state showing this client's user as `me` is to be taken:
    /// one the host published after it handled every intent this client
    /// sent. An earlier one does not show what this user did. The wait
    /// ends without a timer: an intent reaches the host's core, or the
    /// session ends.
    pub fn admits(&self, me: Option<&User>) -> bool {
        me.map_or(0, |me| me.handled) >= self.sent
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

    /// This client's user as the host shows it after `handled` intents.
    fn me(handled: u64) -> User {
        User {
            handled,
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
    fn every_state_is_taken_while_the_host_handled_all_this_client_sent() {
        let mut sync = ClientSync::default();
        assert!(sync.admits(None), "nothing sent, and not in the room yet");

        sync.sent = 3;
        assert!(sync.admits(Some(&me(3))));
        assert!(sync.admits(Some(&me(3))), "a heartbeat");
    }

    #[test]
    fn a_state_from_before_the_host_handled_an_intent_is_ignored() {
        let sync = ClientSync {
            sent: 3,
            ..Default::default()
        };
        assert!(!sync.admits(None), "the host does not know this user yet");
        assert!(!sync.admits(Some(&me(1))));
        assert!(!sync.admits(Some(&me(2))), "only the first two handled");
        assert!(sync.admits(Some(&me(3))));
    }

    #[test]
    fn an_unhandled_intent_never_expires() {
        let sync = ClientSync {
            sent: 1,
            ..Default::default()
        };
        for _ in 0..100 {
            assert!(!sync.admits(Some(&me(0))));
        }
    }
}
