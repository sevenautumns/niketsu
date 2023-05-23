use anyhow::Result;
use enum_dispatch::enum_dispatch;

use super::CoreRunner;
// TODO remove this import somehow
use crate::client::database::message::*;

#[enum_dispatch]
pub trait CoreMessageTrait {
    fn handle(self, client: &mut CoreRunner) -> Result<()>;
}
