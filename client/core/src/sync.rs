//! Client-side inputs for following the host's clock: how far the host's
//! reported position lags behind by the time it arrives, and whether a seek
//! is still settling.

use std::collections::VecDeque;
use std::time::Duration;

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

/// What a client needs to judge the host's heartbeats.
#[derive(Debug, Default, Clone)]
pub struct ClientSync {
    pub delay: HostDelay,
    /// Set when a seek arrives from the room. Until the local player has
    /// actually moved, its position says nothing about where the seek
    /// landed, and a host heartbeat sent before the host applied the seek
    /// would drag us back; host heartbeats are ignored meanwhile.
    seeking: bool,
    /// The local position seen by the last heartbeat that observed a change.
    last_position: Option<Duration>,
    /// Host heartbeats in a row that named another video than the one
    /// playing here.
    other_video_beats: u8,
}

/// How many host heartbeats in a row have to name another video before a
/// client follows the host to it. A selection and the heartbeats around it
/// cross on the way: the host's next heartbeat or two still name the old
/// video after a client selected a new one, and following those would load
/// the old video again only to drop it a moment later. A video the host
/// names this often is not such a crossing: this client missed the
/// selection or lost a race, and nothing but the heartbeat will tell it.
const FOLLOW_HOST_VIDEO_AFTER: u8 = 3;

impl ClientSync {
    pub fn seek_started(&mut self) {
        self.seeking = true;
    }

    pub fn is_seeking(&self) -> bool {
        self.seeking
    }

    /// Fed with the local position on every heartbeat. A changed position
    /// means the player is running again after a seek; without any position
    /// there is no seek left to settle, and waiting for one to change would
    /// shut the host out for good.
    pub fn observe_local_position(&mut self, position: Option<Duration>) {
        if position.is_none() || position != self.last_position {
            self.seeking = false;
            self.last_position = position;
        }
    }

    /// Fed with whether a host heartbeat named another video than the one
    /// playing here. True once it is time to follow the host to its video,
    /// see [`FOLLOW_HOST_VIDEO_AFTER`].
    pub fn host_plays_other_video(&mut self, other: bool) -> bool {
        if !other {
            self.other_video_beats = 0;
            return false;
        }
        self.other_video_beats += 1;
        if self.other_video_beats < FOLLOW_HOST_VIDEO_AFTER {
            return false;
        }
        self.other_video_beats = 0;
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
    fn seek_gate_opens_once_the_local_position_moves() {
        let mut sync = ClientSync::default();
        sync.observe_local_position(Some(Duration::from_secs(5)));

        sync.seek_started();
        assert!(sync.is_seeking());

        // the player has not moved yet
        sync.observe_local_position(Some(Duration::from_secs(5)));
        assert!(sync.is_seeking());

        sync.observe_local_position(Some(Duration::from_secs(60)));
        assert!(!sync.is_seeking());
    }

    #[test]
    fn the_host_is_followed_to_a_video_it_keeps_naming() {
        let mut sync = ClientSync::default();
        assert!(!sync.host_plays_other_video(true));
        assert!(!sync.host_plays_other_video(true));
        assert!(sync.host_plays_other_video(true));
    }

    #[test]
    fn a_heartbeat_that_crossed_a_selection_is_not_followed() {
        let mut sync = ClientSync::default();
        assert!(!sync.host_plays_other_video(true));
        assert!(!sync.host_plays_other_video(true));
        // the host caught up with the selection
        assert!(!sync.host_plays_other_video(false));
        assert!(!sync.host_plays_other_video(true));
    }

    #[test]
    fn seek_gate_opens_when_nothing_is_loaded() {
        let mut sync = ClientSync::default();
        sync.seek_started();

        sync.observe_local_position(None);
        assert!(!sync.is_seeking(), "an empty player has no seek to settle");
    }
}
