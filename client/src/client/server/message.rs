use std::sync::Arc;

use anyhow::{Error, Result};
use enum_dispatch::enum_dispatch;
use log::{error, trace, warn};

use super::NiketsuMessage;
use crate::client::message::CoreMessageTrait;
use crate::client::server::NiketsuJoin;
use crate::client::CoreRunner;

#[enum_dispatch(CoreMessageTrait)]
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    NiketsuMessage,
    ServerError,
    WsStreamEnded,
    Connected,
    SendFinished,
}

#[derive(Debug, Clone)]
pub struct ServerError(pub Arc<Error>);

impl CoreMessageTrait for ServerError {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        warn!("Server Error: {}", self.0);
        client.messages.push_connection_error(self.0.to_string());
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WsStreamEnded;

impl CoreMessageTrait for WsStreamEnded {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        error!("Websocket ended");
        client.messages.push_disconnected();
        client.ws = client.ws.reboot();
        client.ws_sender.store(Arc::new(client.ws.sender().clone()));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Connected;

impl CoreMessageTrait for Connected {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        trace!("Socket: connected");
        client.ws.sender().send(NiketsuJoin {
            password: client.config.password.clone(),
            room: client.config.room.clone(),
            username: client.config.username.clone(),
        })?;
        client.messages.push_connected();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SendFinished(pub Arc<Result<()>>);

impl CoreMessageTrait for SendFinished {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        trace!("{:?}", self.0);
        Ok(())
    }
}
