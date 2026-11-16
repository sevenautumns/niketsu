//! How far this core's player is from the room while both play.
//!
//! Players at the same speed do not drift apart. An offset appears when
//! one of them stalls or lands a seek late, and stays from then on. What
//! changes from one measurement to the next is the measurement: a room
//! state that was underway for longer than usual shows the room further
//! back than it is, never further ahead. So the smallest of the offsets
//! measured over a few seconds is the one closest to the truth, and the
//! tolerance has to cover no more than what is left of the noise.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

/// How long offsets are measured before the player is judged by them.
const WINDOW: Duration = Duration::from_secs(3);

/// How far a playing player may be from the room. Correcting it is a seek
/// its user sees, so it is left alone within this.
const TOLERANCE: Duration = Duration::from_millis(500);

/// How close to a position the player is to be when the room jumped there
/// or is paused there. Wide enough that the player whose own seek moved the
/// room is not made to seek a second time.
pub const ALIGNMENT: Duration = Duration::from_millis(200);

/// The offsets measured since the player was last put where the room is,
/// or since it last had no position.
#[derive(Debug, Clone, Default)]
pub struct Offset {
    /// When the first of these measurements was taken.
    since: Option<Instant>,
    /// When the player was how many seconds ahead of the room, of the last
    /// [`WINDOW`].
    measured: VecDeque<(Instant, f64)>,
}

impl Offset {
    /// What was measured says nothing about the player from here on.
    pub fn restart(&mut self) {
        *self = Self::default();
    }

    /// The player is at `player` while the room is at `room` and plays at
    /// `speed`. Returns whether the player is to be put where the room is.
    ///
    /// Positions are media time and the tolerance is what a viewer
    /// perceives: at double speed a gap closes twice as fast, so twice as
    /// much of it is tolerable.
    pub fn measure(&mut self, player: Duration, room: Duration, speed: f64, now: Instant) -> bool {
        let ahead = player.as_secs_f64() - room.as_secs_f64();
        self.measured.retain(|(at, _)| now - *at <= WINDOW);
        if self.measured.is_empty() {
            // one measurement is no more than the noise it may be
            self.since = Some(now);
        }
        self.measured.push_back((now, ahead));
        if self.since.is_some_and(|since| now - since < WINDOW) {
            return false;
        }
        let least = self.measured.iter().map(|(_, ahead)| *ahead);
        let least = least.fold(f64::INFINITY, f64::min);
        least.abs() > TOLERANCE.as_secs_f64() * speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Duration = Duration::from_secs(100);
    const TICK: Duration = Duration::from_millis(500);

    fn ahead(secs: f64) -> Duration {
        ROOM + Duration::from_secs_f64(secs)
    }

    fn behind(secs: f64) -> Duration {
        ROOM - Duration::from_secs_f64(secs)
    }

    /// An offset and the time its measurements are taken at.
    struct Measured {
        offset: Offset,
        now: Instant,
    }

    impl Measured {
        fn new() -> Self {
            Self {
                offset: Offset::default(),
                now: Instant::now(),
            }
        }

        /// Measures each of `players` half a second after the one before,
        /// and returns what each measurement said.
        fn measure(&mut self, players: &[Duration], speed: f64) -> Vec<bool> {
            let mut verdicts = vec![];
            for player in players {
                let off = self.offset.measure(*player, ROOM, speed, self.now);
                verdicts.push(off);
                self.now += TICK;
            }
            verdicts
        }
    }

    #[test]
    fn a_player_within_the_tolerance_is_left_alone() {
        let mut measured = Measured::new();
        for player in [ahead(0.4), behind(0.4), ROOM] {
            let verdicts = measured.measure(&[player; 20], 1.0);
            assert!(!verdicts.contains(&true), "at {player:?}");
        }
    }

    #[test]
    fn a_player_that_is_off_is_corrected_once_the_window_has_passed() {
        for player in [ahead(0.8), behind(0.8)] {
            let verdicts = Measured::new().measure(&[player; 7], 1.0);
            assert_eq!(
                verdicts,
                [false, false, false, false, false, false, true],
                "at {player:?}"
            );
        }
    }

    /// A state that arrives late shows the room further back, and so the
    /// player further ahead than it is.
    #[test]
    fn late_states_do_not_move_a_player_that_is_level() {
        let mut players = [ahead(2.0); 40];
        for on_time in players.iter_mut().step_by(5) {
            *on_time = ahead(0.05);
        }
        let verdicts = Measured::new().measure(&players, 1.0);
        assert!(!verdicts.contains(&true));
    }

    /// The measurements from before a player stalled do not hide the stall.
    #[test]
    fn a_player_that_fell_behind_is_corrected_though_it_was_level_before() {
        let mut measured = Measured::new();
        assert!(!measured.measure(&[ROOM; 10], 1.0).contains(&true));
        assert_eq!(measured.measure(&[behind(4.0); 2], 1.0), [true, true]);
    }

    /// A player that looks ahead may only look so, until every measurement
    /// of the window says it is.
    #[test]
    fn a_player_that_got_ahead_is_corrected_when_no_measurement_says_otherwise() {
        let mut measured = Measured::new();
        measured.measure(&[ROOM; 10], 1.0);
        let verdicts = measured.measure(&[ahead(4.0); 8], 1.0);
        assert_eq!(verdicts.iter().position(|off| *off), Some(6));
    }

    #[test]
    fn nothing_measured_before_a_restart_counts() {
        let mut measured = Measured::new();
        measured.measure(&[behind(4.0); 6], 1.0);
        measured.offset.restart();
        let verdicts = measured.measure(&[behind(4.0); 6], 1.0);
        assert!(!verdicts.contains(&true));
    }

    /// The first state after the room was not heard of for a while is as
    /// likely late as any.
    #[test]
    fn a_measurement_alone_after_a_silence_decides_nothing() {
        let mut measured = Measured::new();
        measured.measure(&[ROOM; 10], 1.0);
        measured.now += 2 * WINDOW;
        let verdicts = measured.measure(&[ahead(4.0); 7], 1.0);
        assert_eq!(verdicts.iter().position(|off| *off), Some(6));
    }

    #[test]
    fn the_tolerance_scales_with_the_speed() {
        let verdicts = Measured::new().measure(&[behind(0.8); 8], 2.0);
        assert!(!verdicts.contains(&true));
        let verdicts = Measured::new().measure(&[behind(1.2); 8], 2.0);
        assert!(verdicts.contains(&true));
    }
}
