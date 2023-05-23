use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::FutureExt;
use iced::Command;
use iced_native::command::Action;
use log::debug;
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use tokio::sync::Notify;

use super::message::ClientMessage;
use crate::client::server::ServerMessage;
use crate::client::{ClientInner, PlayerMessage};
use crate::iced_window::message::PlayerChanged;
use crate::iced_window::MainMessage;

#[derive(Debug, Clone, Copy)]
pub struct Heartbeat;

impl Heartbeat {
    pub fn start(client_sender: MpscSender<PlayerMessage>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                client_sender.send(Heartbeat.into()).ok();
            }
        });
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

impl ClientMessage for Heartbeat {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        debug!("Heartbeat");
        let playing = client.player.playing_file();
        client.ws.load().send(ServerMessage::VideoStatus {
            filename: playing.as_ref().map(|p| p.video.as_str().to_string()),
            position: playing.and_then(|_| client.player.get_position().ok()),
            paused: client.player.is_paused()?,
            speed: client.player.get_speed()?,
        })
    }
}
