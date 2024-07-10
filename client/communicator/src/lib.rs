use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use log::warn;
use niketsu_core::communicator::*;
use p2p::P2PClient;

use self::messages::NiketsuMessage;

pub mod messages;
pub mod p2p;

#[derive(Debug, Default)]
pub struct WebsocketCommunicator {
    connect_task: Option<tokio::task::JoinHandle<P2PClient>>,
    sender_receiver: Option<P2PClient>,
    endpoint: Option<EndpointInfo>,
    in_queue: VecDeque<IncomingMessage>,
}

impl WebsocketCommunicator {
    async fn receive_niketsu_message(&mut self) -> Result<NiketsuMessage> {
        if let Some(task) = &mut self.connect_task {
            self.sender_receiver
                .replace(task.await.expect("get p2p task failed"));
            self.connect_task.take();
        }
        let Some(sender) = &mut self.sender_receiver else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            bail!("No sender")
        };

        loop {
            //TODO: what?
            if let Some(msg) = sender.next().await {
                return Ok(msg);
            }
            bail!("Websocket ended")
        }
    }

    async fn receive_incoming_message(&mut self) -> Result<IncomingMessage> {
        loop {
            let msg = self.receive_niketsu_message().await?;
            if let ping @ NiketsuMessage::Ping(_) = msg {
                self.sender_receiver
                    .as_mut()
                    .unwrap()
                    .send(ping)
                    .expect("can not send message");
                continue;
            }
            return msg
                .try_into()
                .map_err(|msg| anyhow!("unexpected message: {msg:?}"));
        }
    }
}

#[async_trait]
impl CommunicatorTrait for WebsocketCommunicator {
    fn connect(&mut self, endpoint: EndpointInfo) {
        let client = tokio::time::timeout(
            Duration::from_secs(5),
            P2PClient::new(
                endpoint.room.clone(),
                endpoint.password.clone(),
                endpoint.secure,
            ),
        );
        let connect_task = tokio::task::spawn(async move {
            client
                .await
                .map_err(|_| anyhow::anyhow!("Connection timeout"))
                .expect("timeouted p2p connect")
                .map_err(anyhow::Error::from)
                .expect("failed p2p connect")
        });
        self.endpoint.replace(endpoint);
        self.connect_task.replace(connect_task);
    }

    fn send(&mut self, msg: OutgoingMessage) {
        let Some(sender) = &mut self.sender_receiver else {
            warn!("message dropped: {msg:?}");
            return;
        };
        sender.send(msg.into()).unwrap();
    }

    async fn receive(&mut self) -> IncomingMessage {
        loop {
            if let Some(msg) = self.in_queue.pop_front() {
                return msg;
            }
            if let Ok(msg) = self.receive_incoming_message().await {
                return msg;
            }
        }
    }
}

#[cfg(test)]
mod test {}
