use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;

use super::{MediaPlayerTrait, Source};
use crate::{MediaPlayerEvent, PlayerPositionChange};

/// How far the local position may drift from the room's before it is
/// corrected. Wide enough to swallow ping jitter and short stalls:
/// correcting is more disruptive than the drift it removes, so it has to
/// stay rare.
const TOLERANCE: Duration = Duration::from_millis(2500);

/// How close to a position the player is to be when the room jumped there
/// or is paused there. Wide enough that the player whose own seek moved the
/// room is not made to seek a second time.
const ALIGNMENT: Duration = Duration::from_millis(200);

/// Past this the local player is not drifting, it is stuck (buffering, a slow
/// stream). Jumping forward would skip content nobody has seen, so the room is
/// asked to wait instead.
const MAXIMUM_DELAY: Duration = Duration::from_secs(20);

#[derive(Debug)]
pub struct MediaPlayerWrapper {
    player: Box<dyn MediaPlayerTrait>,
    events: VecDeque<MediaPlayerEvent>,
}

impl MediaPlayerWrapper {
    pub fn new(player: Box<dyn MediaPlayerTrait>) -> Self {
        Self {
            player,
            events: Default::default(),
        }
    }

    /// Puts the player at `pos` unless it is there already.
    pub fn align(&mut self, pos: Duration) {
        let Some(current_pos) = self.player.get_position() else {
            return;
        };
        if current_pos.abs_diff(pos) > ALIGNMENT {
            self.player.set_position(pos);
        }
    }

    /// Corrects the player if it drifted too far from `pos`, where the room
    /// is while it plays at `speed`.
    pub fn reconcile(&mut self, pos: Duration, speed: f64) {
        let Some(current_pos) = self.player.get_position() else {
            return;
        };

        // positions are media time, the thresholds are what a viewer
        // perceives: at double speed a gap closes twice as fast, so twice
        // as much of it is tolerable
        let tolerance = TOLERANCE.mul_f64(speed);
        let max_delay = MAXIMUM_DELAY.mul_f64(speed);

        match current_pos {
            d if d <= pos.saturating_add(tolerance) && d >= pos.saturating_sub(tolerance) => {
                // in sync: leave the player alone
            }
            d if d <= pos.saturating_sub(max_delay) => {
                self.events.push_back(PlayerPositionChange::new(d).into())
            }
            _ => self.player.set_position(pos),
        }
    }
}

#[async_trait]
impl MediaPlayerTrait for MediaPlayerWrapper {
    fn start(&mut self) {
        self.player.start()
    }

    fn pause(&mut self) {
        self.player.pause()
    }

    fn is_paused(&self) -> Option<bool> {
        self.player.is_paused()
    }

    fn set_speed(&mut self, speed: f64) {
        self.player.set_speed(speed)
    }

    fn set_position(&mut self, pos: Duration) {
        self.player.set_position(pos)
    }

    fn get_position(&mut self) -> Option<Duration> {
        self.player.get_position()
    }

    fn load(&mut self, source: Source, pos: Duration) {
        self.player.load(source, pos)
    }

    fn unload(&mut self) {
        self.player.unload()
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        if let Some(event) = self.events.pop_front() {
            return event;
        }
        self.player.event().await
    }
}
