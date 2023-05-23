use anyhow::Result;
use enum_dispatch::enum_dispatch;

use super::ClientInner;
// TODO remove this import somehow
use crate::client::database::message::*;

#[enum_dispatch]
pub trait ClientMessage {
    fn handle(self, client: &mut ClientInner) -> Result<()>;
}
