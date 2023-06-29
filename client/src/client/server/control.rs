use std::time::Duration;

use actix::{AsyncContext, Handler, WrapFuture};
use async_tungstenite::tungstenite::Message as TsMessage;
use futures::SinkExt;
use log::{error, warn};

use super::actor::SocketActor;
use super::message::WsStreamEnded;
use super::NiketsuMessage;

impl Handler<NiketsuMessage> for SocketActor {
    type Result = ();

    fn handle(&mut self, msg: NiketsuMessage, ctx: &mut Self::Context) -> Self::Result {
        let Some(sender) = self.sender.clone() else {
            return;
        };
        ctx.spawn(
            async move {
                match serde_json::to_string(&msg) {
                    Ok(msg) => {
                        if let Err(err) = sender.lock().await.send(TsMessage::Text(msg)).await {
                            warn!("Websocket ended: {err:?}");
                        }
                    }
                    Err(err) => error!("Serder Error: {err:?}"),
                }
            }
            .into_actor(self),
        );
    }
}

impl Handler<WsStreamEnded> for SocketActor {
    type Result = ();

    fn handle(&mut self, _: WsStreamEnded, ctx: &mut Self::Context) -> Self::Result {
        self.reconnect(ctx);
    }
}
