use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use iced::Command;
use iced_native::command::Action;
use log::debug;
use tokio::sync::Notify;
use tokio::time::Interval;

use super::message::CoreMessageTrait;
use super::CoreRunner;
use crate::client::server::NiketsuVideoStatus;
use crate::iced_window::message::PlayerChanged;
use crate::iced_window::MainMessage;

#[derive(Debug, Clone, Copy)]
pub struct Heartbeat;

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

pub struct Changed;

impl Changed {
    pub fn next(notify: Arc<Notify>) -> Command<MainMessage> {
        async fn changed(notify: Arc<Notify>) -> MainMessage {
            notify.notified().await;
            PlayerChanged.into()
        }
        Command::single(Action::Future(changed(notify).boxed()))
    }
}

impl CoreMessageTrait for Heartbeat {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        debug!("Heartbeat");
        let playing = client.player.playing_file();
        client.ws.sender().send(NiketsuVideoStatus {
            filename: playing.as_ref().map(|p| p.video.as_str().to_string()),
            position: playing.and_then(|_| client.player.get_position().ok()),
            paused: client.player.is_paused()?,
            speed: client.player.get_speed()?,
        })
    }
}
