use std::time::Duration;

use anyhow::Result;
use enum_dispatch::enum_dispatch;

use super::message::ClientMessage;
use super::ClientInner;
use crate::video::{PlayingFile, Video};

#[enum_dispatch(ClientMessage)]
#[derive(Debug, Clone)]
pub enum UiMessage {
    MpvSelect,
}

#[derive(Debug, Clone)]
pub struct MpvSelect(pub Video);

impl ClientMessage for MpvSelect {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        let file = PlayingFile {
            video: self.0,
            paused: true,
            speed: client.player.get_speed()?,
            pos: Duration::ZERO,
        };
        client.player.load(file)
    }
}
