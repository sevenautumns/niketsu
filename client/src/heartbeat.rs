use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use iced::Command;
use iced_native::command::Action;
use log::debug;
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use tokio::sync::Notify;

use crate::client::{ClientInner, PlayerMessage};
use crate::window::MainMessage;
use crate::ws::ServerMessage;

pub struct Heartbeat;

impl Heartbeat {
    pub fn start(client_sender: MpscSender<PlayerMessage>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                client_sender.send(PlayerMessage::Heartbeat).ok();
            }
        });
    }
}

impl ClientInner {
    pub fn react_to_heartbeat(&mut self) -> Result<()> {
        debug!("Heartbeat");
        let playing = self.mpv.playing();
        self.ws.load().send(ServerMessage::VideoStatus {
            filename: playing.as_ref().map(|p| p.video.as_str().to_string()),
            position: playing.and_then(|_| self.mpv.get_playback_position().ok()),
            paused: self.mpv.get_pause_state(),
            speed: self.mpv.speed(),
        })
    }
}

pub struct Changed;

impl Changed {
    pub fn next(notify: Arc<Notify>) -> Command<MainMessage> {
        async fn changed(notify: Arc<Notify>) -> MainMessage {
            notify.notified().await;
            MainMessage::PlayerChanged
        }
        Command::single(Action::Future(changed(notify).boxed()))
    }
}
