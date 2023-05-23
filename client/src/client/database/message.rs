use anyhow::Result;
use enum_dispatch::enum_dispatch;
use log::{debug, trace};

use crate::client::message::ClientMessage;
use crate::client::ClientInner;

#[enum_dispatch(ClientMessage)]
#[derive(Debug, Clone)]
pub enum DatabaseMessage {
    Changed,
    UpdateFinished,
}

#[derive(Debug, Clone)]
pub struct Changed;

impl ClientMessage for Changed {
    fn handle(self, client: &mut ClientInner) -> Result<()> {
        trace!("Database: changed");
        client.player.reload()
    }
}

#[derive(Debug, Clone)]
pub struct UpdateFinished;

impl ClientMessage for UpdateFinished {
    fn handle(self, _: &mut ClientInner) -> Result<()> {
        debug!("Database: update finished");
        Ok(())
    }
}
