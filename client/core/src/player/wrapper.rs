use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;

use super::MediaPlayerTrait;
use crate::playlist::Video;
use crate::{FilePathSearch, FileStore, MediaPlayerEvent, PlayerPositionChange};

/// How far the local position may drift from the host before it is corrected.
/// Wide enough to swallow ping jitter and short stalls: correcting is more
/// disruptive than the drift it removes, so it has to stay rare.
const TOLERANCE: Duration = Duration::from_millis(2500);

/// Past this the local player is not drifting, it is stuck (buffering, a slow
/// stream). Jumping forward would skip content nobody has seen, so the room is
/// asked to wait instead.
const MAXIMUM_DELAY: Duration = Duration::from_secs(20);

#[derive(Debug)]
pub struct MediaPlayerWrapper {
    player: Box<dyn MediaPlayerTrait>,
    // keep track of the speed the host dictates for the room
    host_speed: f64,
    events: VecDeque<MediaPlayerEvent>,
}

impl MediaPlayerWrapper {
    pub fn new(player: Box<dyn MediaPlayerTrait>) -> Self {
        Self {
            player,
            host_speed: 1.0,
            events: Default::default(),
        }
    }

    pub fn reconcile(&mut self, pos: Duration) {
        let Some(current_pos) = self.player.get_position() else {
            return;
        };

        // positions are media time, the thresholds are what a viewer
        // perceives: at double speed a gap closes twice as fast, so twice
        // as much of it is tolerable
        let tolerance = TOLERANCE.mul_f64(self.host_speed);
        let max_delay = MAXIMUM_DELAY.mul_f64(self.host_speed);

        match current_pos {
            d if d <= pos.saturating_add(tolerance) && d >= pos.saturating_sub(tolerance) => {
                // in sync: leave the player alone
            }
            d if d <= pos.saturating_sub(max_delay) => self
                .events
                .push_back(PlayerPositionChange { pos: d }.into()),
            _ => self.player.set_position(pos),
        }
    }

    /// The local player already runs at `speed` (the change originated
    /// there); only the reference speed needs to follow.
    pub fn sync_host_speed(&mut self, speed: f64) {
        self.host_speed = speed;
    }

    /// The speed the room plays at — what the host announces and every other
    /// peer adopts.
    pub fn host_speed(&self) -> f64 {
        self.host_speed
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

    /// The host repeats the room's speed with every heartbeat, so following
    /// it has to be free when nothing changed.
    fn set_speed(&mut self, speed: f64) {
        if self.host_speed == speed {
            return;
        }
        self.host_speed = speed;
        self.player.set_speed(speed)
    }

    fn set_position(&mut self, pos: Duration) {
        self.player.set_position(pos)
    }

    fn get_position(&mut self) -> Option<Duration> {
        self.player.get_position()
    }

    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore) {
        self.player.load_video(load, pos, db)
    }

    fn unload_video(&mut self) {
        self.player.unload_video()
    }

    fn maybe_reload_video(&mut self, f: &dyn FilePathSearch) {
        self.player.maybe_reload_video(f)
    }

    fn reload_video(&mut self, f: &dyn FilePathSearch, filename: &str) {
        self.player.reload_video(f, filename)
    }

    fn playing_video(&self) -> Option<Video> {
        self.player.playing_video()
    }

    fn video_loaded(&self) -> bool {
        self.player.video_loaded()
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        if let Some(event) = self.events.pop_front() {
            return event;
        }
        self.player.event().await
    }
}
