use std::time::Duration;

use tokio::time::Interval;

use super::communicator::NiketsuVideoStatus;
use super::{CoreModel, EventHandler};

pub struct Pacemaker {
    interval: Interval,
}

impl Default for Pacemaker {
    fn default() -> Self {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self { interval }
    }
}

impl Pacemaker {
    pub async fn recv(&mut self) -> Heartbeat {
        self.interval.tick().await;
        Heartbeat
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Heartbeat;

impl EventHandler for Heartbeat {
    fn handle(self, model: &mut CoreModel) {
        let filename = model
            .player
            .playing_video()
            .map(|v| v.name_str().to_string());
        let position = model.player.get_position();
        let speed = model.player.get_speed();
        let paused = model.player.is_paused().unwrap_or(true);
        model.communicator.send(
            NiketsuVideoStatus {
                filename,
                position,
                speed,
                paused,
            }
            .into(),
        );
    }
}
