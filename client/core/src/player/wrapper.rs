use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;
use log::trace;

use super::MediaPlayerTrait;
use crate::playlist::Video;
use crate::{FileStore, MediaPlayerEvent, PlayerPositionChange};

const MAXIMUM_DELAY: Duration = Duration::from_secs(5);

const MINIMUM_DELAY: Duration = Duration::from_secs(1);

const MAXIMUM_SPEED_DIFF: f64 = 0.05;

const MINIMUM_SPEED_DIFF: f64 = 0.02;

#[derive(Debug)]
pub struct MediaPlayerWrapper {
    player: Box<dyn MediaPlayerTrait>,
    // keep track of speed of host to allow for divergent client speed for ketchup
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

        //TODO refactor
        // flexible delay based on current speed
        let current_speed = self.player.get_speed();
        let min_delay = MINIMUM_DELAY.mul_f64(self.host_speed);
        let max_delay = MAXIMUM_DELAY.mul_f64(self.host_speed);

        match current_pos {
            d if d <= pos.saturating_add(min_delay) && d >= pos.saturating_sub(min_delay) => {
                if current_speed != self.host_speed {
                    self.player.set_speed(self.host_speed);
                }
            }
            d if d >= pos.saturating_add(max_delay) => {
                self.player.set_position(pos);
            }
            d if d <= pos.saturating_sub(max_delay) => self
                .events
                .push_back(PlayerPositionChange { pos: d }.into()),
            d if d > pos.saturating_add(min_delay) || d < pos.saturating_sub(min_delay) => {
                self.stepwise_speed_change(d, pos)
            }
            _ => trace!("position should not be possible"),
        }
    }

    fn stepwise_speed_change(&mut self, client_pos: Duration, host_pos: Duration) {
        let speed_increase: f64;

        if host_pos > client_pos {
            let diff = host_pos.saturating_sub(client_pos);
            speed_increase = diff.as_secs_f64() * (MAXIMUM_SPEED_DIFF - MINIMUM_SPEED_DIFF) / 4.0;
        } else {
            let diff = host_pos.saturating_sub(client_pos);
            speed_increase =
                (-1.0) * diff.as_secs_f64() * (MAXIMUM_SPEED_DIFF - MINIMUM_SPEED_DIFF) / 4.0;
        }
        self.player
            .set_speed(self.host_speed * (1.0 + speed_increase));
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
        let diff = self.host_speed - self.player.get_speed();
        self.host_speed = speed;
        self.player.set_speed(speed + diff)
    }

    fn get_speed(&self) -> f64 {
        self.player.get_speed()
    }

    fn set_position(&mut self, pos: Duration) {
        self.player.set_position(pos)
    }

    fn get_position(&mut self) -> Option<Duration> {
        self.player.get_position()
    }

    fn cache_available(&mut self) -> bool {
        self.player.cache_available()
    }

    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore) {
        self.player.load_video(load, pos, db)
    }

    fn unload_video(&mut self) {
        self.player.unload_video()
    }

    fn maybe_reload_video(&mut self, db: &FileStore) {
        self.player.maybe_reload_video(db)
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
