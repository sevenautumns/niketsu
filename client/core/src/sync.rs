//! Judging the room's state as it reaches a client: which state is newer,
//! when this client's own intents are still on their way, and how far the
//! host's clock lags behind by the time it arrives. On the host's side, the
//! position it reports while its own player is still jumping.

use std::collections::VecDeque;
use std::time::Duration;

use crate::communicator::RoomStateMsg;

/// Number of round-trip samples the delay estimate is taken over. The
/// communicator pings the host once a second, so this covers the last ten.
const DELAY_WINDOW: usize = 10;

/// How many newer states a client ignores while an intent of its own is
/// pending. The host publishes with every heartbeat, so this bounds how
/// long an intent the host never handled keeps the client apart from the
/// room.
const PENDING_STATES: u8 = 4;

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

/// Intents this client sent that the host has not answered yet. The local
/// player already did what they ask, and a state the host published before
/// handling them would undo it.
#[derive(Debug, Clone, Copy)]
struct Pending {
    /// The first version that can include every one of them: the host
    /// bumps the version once for each intent it handles.
    until: u64,
    /// Newer states still to ignore before giving up on them.
    states: u8,
}

/// What a client needs to judge the host's room states. Belongs to one
/// connection: a new host counts its versions anew.
#[derive(Debug, Default, Clone)]
pub struct ClientSync {
    pub delay: HostDelay,
    /// The counters of the state applied last.
    applied: Option<Counters>,
    pending: Option<Pending>,
}

#[derive(Debug, Clone, Copy)]
struct Counters {
    version: u64,
    beat: u64,
    jumps: u64,
}

impl From<&RoomStateMsg> for Counters {
    fn from(state: &RoomStateMsg) -> Self {
        Self {
            version: state.version,
            beat: state.beat,
            jumps: state.jumps,
        }
    }
}

/// How an admitted state differs from the one applied before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    /// The position jumped, so it is to be taken exactly.
    pub jumped: bool,
    /// A new version, not only a heartbeat: what is not repeated with
    /// every heartbeat, like the user list, may have changed.
    pub changed: bool,
}

impl ClientSync {
    /// Notes an intent sent to the host while the state applied here is
    /// the latest known.
    pub fn intent_sent(&mut self) {
        let applied = self.applied.map_or(0, |applied| applied.version);
        let sent_before = self.pending.map_or(applied, |p| p.until.max(applied));
        self.pending = Some(Pending {
            until: sent_before + 1,
            states: PENDING_STATES,
        });
    }

    /// Whether `state` is to be applied, and how it differs from the state
    /// applied before. A state no newer than the applied one is dropped; so
    /// is one that predates the pending intents, until they expire.
    pub fn admit(&mut self, state: &RoomStateMsg) -> Option<Admitted> {
        if let Some(applied) = self.applied
            && state.order() <= (applied.version, applied.beat)
        {
            return None;
        }
        if let Some(pending) = &mut self.pending
            && state.version < pending.until
            && pending.states > 0
        {
            pending.states -= 1;
            return None;
        }
        self.pending = None;
        let admitted = Admitted {
            jumped: self.applied.is_none_or(|a| a.jumps != state.jumps),
            changed: self.applied.is_none_or(|a| a.version != state.version),
        };
        self.applied = Some(state.into());
        Some(admitted)
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

/// A jump of the host's player to `target`. Until the player has moved, its
/// position says nothing about where the jump lands, and the room is told
/// the target instead.
#[derive(Debug, Clone, Copy)]
pub struct Jump {
    target: Duration,
    /// What the player reported right after it was told to jump.
    before: Option<Duration>,
}

impl Jump {
    pub fn new(target: Duration, before: Option<Duration>) -> Self {
        Self { target, before }
    }

    /// The position to tell the room while the player reports `reported`,
    /// or `None` once the player has moved and its own position counts. A
    /// player without any position has nothing left to land.
    pub fn position(&self, reported: Option<Duration>) -> Option<Duration> {
        (reported.is_some() && reported == self.before).then_some(self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(version: u64, beat: u64) -> RoomStateMsg {
        RoomStateMsg {
            version,
            beat,
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
    fn only_a_newer_state_is_applied() {
        let mut sync = ClientSync::default();
        assert!(sync.admit(&state(3, 7)).is_some());
        assert!(sync.admit(&state(3, 7)).is_none(), "the same state again");
        assert!(sync.admit(&state(3, 6)).is_none(), "an older heartbeat");
        assert!(sync.admit(&state(2, 9)).is_none(), "an older version");
        assert!(sync.admit(&state(3, 8)).is_some());
        assert!(sync.admit(&state(4, 8)).is_some());
    }

    #[test]
    fn a_state_from_before_the_hosts_answer_is_ignored() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3, 7));
        sync.intent_sent();

        assert!(sync.admit(&state(3, 8)).is_none());
        assert!(sync.admit(&state(4, 9)).is_some(), "the host answered");
        assert!(sync.admit(&state(4, 10)).is_some());
    }

    #[test]
    fn every_pending_intent_has_to_be_answered() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3, 7));
        sync.intent_sent();
        sync.intent_sent();

        assert!(
            sync.admit(&state(4, 7)).is_none(),
            "only the first answered"
        );
        assert!(sync.admit(&state(5, 7)).is_some());
    }

    #[test]
    fn an_unanswered_intent_expires() {
        let mut sync = ClientSync::default();
        sync.admit(&state(3, 0));
        sync.intent_sent();

        for beat in 1..=u64::from(PENDING_STATES) {
            assert!(sync.admit(&state(3, beat)).is_none());
        }
        assert!(sync.admit(&state(3, 9)).is_some(), "the intent was lost");
        assert!(sync.admit(&state(3, 10)).is_some());
    }

    #[test]
    fn a_jump_or_a_new_version_is_reported() {
        let mut sync = ClientSync::default();
        let first = sync.admit(&state(1, 0)).unwrap();
        assert!(first.jumped && first.changed, "nothing applied yet");

        let beat = sync.admit(&state(1, 1)).unwrap();
        assert!(!beat.jumped && !beat.changed);

        let mut seek = state(2, 1);
        seek.jumps = 1;
        let seek = sync.admit(&seek).unwrap();
        assert!(seek.jumped && seek.changed);
    }

    #[test]
    fn a_jump_reports_its_target_until_the_player_moves() {
        let jump = Jump::new(Duration::from_secs(300), Some(Duration::from_secs(10)));
        assert_eq!(
            jump.position(Some(Duration::from_secs(10))),
            Some(Duration::from_secs(300))
        );
        assert_eq!(jump.position(Some(Duration::from_secs(300))), None);
        assert_eq!(jump.position(None), None, "an empty player lands nothing");
    }
}
