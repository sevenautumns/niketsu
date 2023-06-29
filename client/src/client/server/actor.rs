use std::sync::Arc;
use std::time::Duration;

use actix::{Actor, ActorFutureExt, AsyncContext, Context, SpawnHandle, WrapFuture};
use anyhow::{bail, Result};
use async_tungstenite::stream::Stream;
use async_tungstenite::tokio::TokioAdapter;
use async_tungstenite::tungstenite::Message as TsMessage;
use async_tungstenite::WebSocketStream;
use futures::stream::{SplitSink, SplitStream};
use futures::StreamExt;
use log::error;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_native_tls::TlsStream;
use url::Url;

use super::NiketsuMessage;
use crate::client::server::message::WsStreamEnded;

type WsSink = SplitSink<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
    TsMessage,
>;
type WsStream = SplitStream<
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>,
>;

pub struct SocketActor {
    pub(super) url: Url,
    pub(super) sender: Option<Arc<Mutex<WsSink>>>,
    pub(super) receiver: Option<SpawnHandle>,
}

impl Actor for SocketActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.reconnect(ctx);
    }
}

impl SocketActor {
    pub(super) fn reconnect(&mut self, ctx: &mut Context<Self>) {
        self.sender = None;
        if let Some(recv) = self.receiver {
            ctx.cancel_future(recv);
        }
        let addr = ctx.address();
        if let Err(err) = self.try_connect(ctx) {
            error!("Reconnect failed: {err:?}");
            ctx.spawn(
                async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    addr.do_send(WsStreamEnded)
                }
                .into_actor(self),
            );
        }
    }

    fn try_connect(&mut self, ctx: &mut Context<Self>) -> Result<()> {
        let addr = self.url.clone();
        ctx.wait(
            async move {
                let connection = async_tungstenite::tokio::connect_async(addr).await;
                if let Ok((ws, _)) = connection {
                    return Some(ws.split());
                }
                // TODO print error in err case
                None
            }
            .into_actor(self)
            .then(|res, s, ctx| {
                if let Some((sink, stream)) = res {
                    s.sender = Some(Arc::new(Mutex::new(sink)));
                    s.spawn_receiver(stream, ctx)
                }
                actix::fut::ready(())
            }),
        );
        if self.sender.is_none() {
            bail!("Did not connect to server")
        }
        Ok(())
    }

    fn spawn_receiver(&mut self, mut stream: WsStream, ctx: &mut Context<Self>) {
        let addr = ctx.address();
        self.receiver = Some(
            ctx.spawn(
                async move {
                    while let Some(Ok(msg)) = stream.next().await {
                        let Ok(msg) = msg.into_text() else {
                            continue;
                        };
                        let Ok(msg) = serde_json::from_str::<NiketsuMessage>(&msg) else {
                            continue;
                        };
                        todo!()
                        // addr.do_send(todo!())
                    }
                    addr.do_send(WsStreamEnded);
                }
                .into_actor(self),
            ),
        );
    }
}
