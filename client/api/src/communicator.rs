use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use niketsu_core::communicator::{CommunicatorTrait, EndpointInfo, IncomingMessage, OutgoingMessage};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub struct CommHandle {
    pub(crate) outgoing_rx: UnboundedReceiver<OutgoingMessage>,
    pub(crate) incoming_tx: UnboundedSender<IncomingMessage>,
    connected: Arc<AtomicBool>,
}

impl CommHandle {
    pub async fn recv_outgoing(&mut self) -> OutgoingMessage {
        self.outgoing_rx.recv().await.expect("ApiCommunicator dropped")
    }

    pub fn recv_outgoing_blocking(&mut self) -> OutgoingMessage {
        self.outgoing_rx.blocking_recv().expect("ApiCommunicator dropped")
    }

    pub fn try_recv_outgoing(&mut self) -> Option<OutgoingMessage> {
        self.outgoing_rx.try_recv().ok()
    }

    pub fn send_incoming(&self, msg: IncomingMessage) {
        let _ = self.incoming_tx.send(msg);
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct ApiCommunicator {
    outgoing_tx: UnboundedSender<OutgoingMessage>,
    incoming_rx: UnboundedReceiver<IncomingMessage>,
    connected: Arc<AtomicBool>,
}

pub fn api_communicator() -> (ApiCommunicator, CommHandle) {
    let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
    let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
    let connected = Arc::new(AtomicBool::new(false));
    (
        ApiCommunicator {
            outgoing_tx,
            incoming_rx,
            connected: Arc::clone(&connected),
        },
        CommHandle {
            outgoing_rx,
            incoming_tx,
            connected,
        },
    )
}

#[async_trait]
impl CommunicatorTrait for ApiCommunicator {
    fn connect(&mut self, _connect: EndpointInfo) {
        self.connected.store(true, Ordering::Relaxed);
    }

    fn send(&mut self, msg: OutgoingMessage) {
        let _ = self.outgoing_tx.send(msg);
    }

    async fn receive(&mut self) -> IncomingMessage {
        self.incoming_rx.recv().await.expect("CommHandle dropped")
    }

    fn has_endpoint(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::communicator::{ConnectedMsg, StartMsg};

    use super::*;

    #[tokio::test]
    async fn send_is_observed_on_handle() {
        let (mut comm, mut handle) = api_communicator();

        comm.send(OutgoingMessage::Start(StartMsg { actor: "test".into() }));

        let msg = handle.recv_outgoing().await;
        assert!(matches!(msg, OutgoingMessage::Start(_)));
    }

    #[tokio::test]
    async fn incoming_from_handle_is_received_by_communicator() {
        let (mut comm, handle) = api_communicator();

        handle.send_incoming(ConnectedMsg { is_host: false }.into());

        let msg = comm.receive().await;
        assert!(matches!(msg, IncomingMessage::Connected(_)));
    }

    #[test]
    fn connect_sets_has_endpoint() {
        let (mut comm, handle) = api_communicator();

        assert!(!comm.has_endpoint());
        assert!(!handle.is_connected());

        comm.connect(EndpointInfo {
            addr: "/ip4/127.0.0.1/tcp/7766".parse().unwrap(),
            room: "room".into(),
            password: String::new(),
        });

        assert!(comm.has_endpoint());
        assert!(handle.is_connected());
    }
}
