use anyhow::Result;
use enum_dispatch::enum_dispatch;
use log::{debug, trace};

use crate::client::message::CoreMessageTrait;
use crate::client::CoreRunner;

#[enum_dispatch(CoreMessageTrait)]
#[derive(Debug, Clone, Copy)]
pub enum DatabaseEvent {
    Changed,
    UpdateFinished,
}

#[derive(Debug, Clone, Copy)]
pub struct Changed;

impl CoreMessageTrait for Changed {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        trace!("Database: changed");
        client.player.reload()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateFinished;

impl CoreMessageTrait for UpdateFinished {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        debug!("Database: update finished");
        Ok(())
    }
}
