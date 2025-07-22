use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::kad::{self};
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, gossipsub, ping};
use niketsu_core::communicator::{
    ConnectedMsg, PlaylistMsg, SeekMsg, SelectMsg, UserStatusMsg, VideoStatusMsg,
};
use niketsu_core::log_err_msg;
use tracing::{debug, error, info, trace, warn};

use super::file_share::FileShareEventHandler;
use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait, MessageResponse,
    SwarmHandler,
};
use crate::messages::NiketsuMessage;
use crate::p2p::MessageRequest;
use crate::p2p::file_share::{FileShareCoreMessageHandler, FileShareSwarmRequestHandler};

#[enum_dispatch]
pub(crate) trait ClientSwarmEventHandler {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler);
}

#[enum_dispatch(ClientSwarmEventHandler)]
enum ClientSwarmEvent {
    Ping(ping::Event),
    Dcutr(dcutr::Event),
    GossipSub(gossipsub::Event),
    MessageRequestResponse(request_response::Event<MessageRequest, MessageResponse>),
    Kademlia(kad::Event),
    ConnectionEstablished(ConnectionEstablished),
    ConnectionClosed(ConnectionClosed),
    OutgoingConnectionError(OutgoingConnectionError),
    Other(Box<SwarmEvent<BehaviourEvent>>),
}

impl ClientSwarmEvent {
    fn from(event: SwarmEvent<BehaviourEvent>) -> Self {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(event)) => ClientSwarmEvent::Ping(event),
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(event)) => ClientSwarmEvent::Dcutr(event),
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(event)) => {
                ClientSwarmEvent::GossipSub(event)
            }
            SwarmEvent::Behaviour(BehaviourEvent::MessageRequestResponse(event)) => {
                ClientSwarmEvent::MessageRequestResponse(event)
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => {
                ClientSwarmEvent::Kademlia(event)
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => ClientSwarmEvent::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
            }),
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => ClientSwarmEvent::ConnectionClosed(ConnectionClosed {
                peer_id,
                cause,
                connection_id,
            }),
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            } => ClientSwarmEvent::OutgoingConnectionError(OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            }),
            _ => ClientSwarmEvent::Other(Box::new(event)),
        }
    }
}

impl ClientSwarmEventHandler for ping::Event {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        debug!("Received ping!");
        if self.peer != handler.handler.host {
            return;
        }

        if let Some(conn) = handler.host_conn {
            if self.connection == conn {
                match self.result {
                    Ok(d) => handler.delay = d,
                    Err(error) => warn!(%error, "Failed to get ping rtt"),
                }
            }
        };
    }
}

impl ClientSwarmEventHandler for dcutr::Event {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        match self.result {
            Ok(res) => {
                handler.host_conn = Some(res);

                info!("Established direct connection. Closing connection to relay");
                if let Some(conn) = handler.relay_conn {
                    handler.handler.swarm.close_connection(conn);
                }

                handler
                    .handler
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&self.remote_peer_id);
            }
            Err(error) => error!(
                %self.remote_peer_id, %error,
                "Direct connection (hole punching) failed"
            ),
        }
    }
}

impl ClientSwarmEventHandler for gossipsub::Event {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        match self {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => {
                debug!(%message_id, %message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                let res = handler.handle_swarm_broadcast(message.data, propagation_source);
                log_err_msg!(res, "Failed to handle broadcast message");
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }
}

impl ClientSwarmEventHandler for request_response::Event<MessageRequest, MessageResponse> {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        match self {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = handler.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    let res = handler.handle_swarm_response(response, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
            },
            request_response::Event::OutboundFailure { peer, error, .. } => {
                let host = handler.handler.host;
                if peer == handler.handler.host {
                    warn!(%error, %peer, %host, "Outbound failure for request response" );
                }
            }
            request_response::Event::ResponseSent {
                peer,
                connection_id,
                request_id,
            } => {
                debug!(
                    ?peer,
                    ?connection_id,
                    ?request_id,
                    "Received response sent event"
                );
            }
            request_response_event => debug!(
                ?request_response_event,
                "Received request response event that is not handled"
            ),
        }
    }
}

impl ClientSwarmEventHandler for kad::Event {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        FileShareEventHandler::handle_event(self, &mut handler.handler);
    }
}

struct ConnectionEstablished {
    peer_id: PeerId,
    connection_id: ConnectionId,
    endpoint: ConnectedPoint,
}

impl ClientSwarmEventHandler for ConnectionEstablished {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        if self.peer_id != handler.handler.host {
            return;
        }

        handler
            .handler
            .swarm
            .behaviour_mut()
            .gossipsub
            .add_explicit_peer(&self.peer_id);

        if self.endpoint.is_relayed() {
            handler.relay_conn = Some(self.connection_id);
        } else {
            info!(%self.connection_id, ?self.endpoint, "Direct connection to host established!");
            handler
                .handler
                .swarm
                .behaviour_mut()
                .kademlia
                .add_address(&self.peer_id, self.endpoint.get_remote_address().clone());
        }
        if let Err(error) = handler.handler.message_sender.send(ConnectedMsg.into()) {
            warn!(%error, "Failed to send connected message to core");
        }
    }
}

struct ConnectionClosed {
    peer_id: PeerId,
    cause: Option<ConnectionError>,
    connection_id: ConnectionId,
}

impl ClientSwarmEventHandler for ConnectionClosed {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        if self.peer_id == handler.handler.host
            && !handler.handler.swarm.is_connected(&self.peer_id)
        {
            warn!(?self.cause, ?self.peer_id, host = %handler.handler.host, %self.connection_id, "Connection to host closed");
            handler.handler.core_receiver.close();
        }
    }
}

struct OutgoingConnectionError {
    connection_id: ConnectionId,
    peer_id: Option<PeerId>,
    error: DialError,
}

impl ClientSwarmEventHandler for OutgoingConnectionError {
    fn handle_swarm_event(self, handler: &mut ClientCommunicationHandler) {
        let Some(pid) = self.peer_id else {
            warn!(%self.error, "Outgoing connection error with unknown peer");
            return;
        };

        if let Some(conn) = handler.host_conn {
            if self.connection_id != conn {
                warn!(%self.error, %self.connection_id, "Outgoing connection error with non-host. Ignoring");
                return;
            }
        }

        if pid == handler.handler.host && !handler.handler.swarm.is_connected(&pid) {
            warn!(?self.error, ?self.peer_id, host = %handler.handler.host, %self.connection_id, "Connection error to host");
            handler.handler.core_receiver.close();
        }
    }
}

impl ClientSwarmEventHandler for Box<SwarmEvent<BehaviourEvent>> {
    fn handle_swarm_event(self, _handler: &mut ClientCommunicationHandler) {
        debug!(event = ?self, "Received not captured event")
    }
}

#[enum_dispatch]
pub(crate) trait ClientCoreMessageHandler {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()>;
}

impl ClientCoreMessageHandler for UserStatusMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let host = handler.handler.host;
        handler.handler.swarm.send_request(&host, self.into());
        Ok(())
    }
}

impl ClientCoreMessageHandler for PlaylistMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let host = handler.handler.host;
        handler.handler.swarm.send_request(&host, self.into());
        Ok(())
    }
}

impl ClientCoreMessageHandler for VideoStatusMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        if self.position != handler.video_status.position {
            handler.is_seeking = false;
            handler.video_status = self;
        }
        Ok(())
    }
}

impl ClientCoreMessageHandler for SelectMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let topic = handler.handler.topic.clone();
        handler.handler.reset_requests_responses();
        handler.handler.swarm.try_broadcast(topic, self.into())
    }
}

impl ClientCoreMessageHandler for NiketsuMessage {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let topic = handler.handler.topic.clone();
        handler.handler.swarm.try_broadcast(topic, self)
    }
}

#[enum_dispatch()]
trait ClientSwarmBroadcastHandler {
    fn handle_swarm_broadcast(
        self,
        peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()>;
}

#[enum_dispatch(ClientSwarmBroadcastHandler)]
enum ClientSwarmBroadcast {
    VideoStatus(VideoStatusMsg),
    Select(SelectMsg),
    Seek(SeekMsg),
    Passthrough(PassthroughMsg),
    Other(NiketsuMessage),
}

impl ClientSwarmBroadcast {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::VideoStatus(msg) => ClientSwarmBroadcast::VideoStatus(msg),
            NiketsuMessage::Seek(msg) => ClientSwarmBroadcast::Seek(msg),
            NiketsuMessage::Select(msg) => ClientSwarmBroadcast::Select(msg),
            NiketsuMessage::Join(_)
            | NiketsuMessage::StatusList(_)
            | NiketsuMessage::Pause(_)
            | NiketsuMessage::Start(_)
            | NiketsuMessage::PlaybackSpeed(_)
            | NiketsuMessage::UserMessage(_)
            | NiketsuMessage::ServerMessage(_) => {
                ClientSwarmBroadcast::Passthrough(PassthroughMsg {
                    niketsu_msg: message,
                })
            }
            msg => ClientSwarmBroadcast::Other(msg),
        }
    }
}

impl ClientSwarmBroadcastHandler for VideoStatusMsg {
    fn handle_swarm_broadcast(
        self,
        peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        if peer_id != handler.handler.host {
            bail!("Received video status from non-host peer: {peer_id:?}")
        }

        if handler.is_seeking {
            debug!("can not determine client position during seek");
            return Ok(());
        }

        let mut video_status = self.clone();
        if let Some(pos) = video_status.position {
            debug!("add delay to position");
            if !video_status.paused {
                video_status.position = Some(pos + handler.delay.div_f64(2.0));
            }
        }

        handler
            .handler
            .message_sender
            .send(video_status.into())
            .map_err(anyhow::Error::from)
    }
}

impl ClientSwarmBroadcastHandler for SelectMsg {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler.handler.reset_requests_responses();
        handler
            .handler
            .message_sender
            .send(self.into())
            .map_err(anyhow::Error::from)
    }
}

impl ClientSwarmBroadcastHandler for SeekMsg {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler.is_seeking = true;
        handler
            .handler
            .message_sender
            .send(self.into())
            .map_err(anyhow::Error::from)
    }
}

struct PassthroughMsg {
    niketsu_msg: NiketsuMessage,
}

impl ClientSwarmBroadcastHandler for PassthroughMsg {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler
            .handler
            .message_sender
            .send(self.niketsu_msg.clone())
            .map_err(anyhow::Error::from)
    }
}

impl ClientSwarmBroadcastHandler for NiketsuMessage {
    fn handle_swarm_broadcast(
        self,
        _peer_id: PeerId,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler
            .handler
            .message_sender
            .send(self)
            .map_err(anyhow::Error::from)
    }
}

pub(crate) struct ClientCommunicationHandler {
    handler: CommunicationHandler,
    host_conn: Option<ConnectionId>,
    relay_conn: Option<ConnectionId>,
    video_status: VideoStatusMsg,
    is_seeking: bool,
    delay: Duration,
}

impl ClientCommunicationHandler {
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        relay_addr: Multiaddr,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        let handler = CommunicationHandler::new(
            swarm,
            topic,
            host,
            relay_addr,
            core_receiver,
            message_sender,
        );
        Self {
            handler,
            host_conn: None,
            relay_conn: None,
            video_status: VideoStatusMsg::default(),
            is_seeking: false,
            delay: Duration::default(),
        }
    }
}

#[async_trait]
impl CommunicationHandlerTrait for ClientCommunicationHandler {
    async fn run(&mut self) {
        loop {
            let base = &mut self.handler.base;
            tokio::select! {
                event = base.swarm.select_next_some() => self.handle_swarm_event(event),
                msg = base.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "Message from core");
                        let res = self.handle_core_message(msg);
                        log_err_msg!(res, "Handling message caused error");
                    },
                    None => {
                        error!("Channel of core closed. Stopping p2p client event loop");
                        break
                    }
                },
            }
        }
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        debug!(?event, host = %self.handler.host, peer = %self.handler.swarm.local_peer_id(), "Handling swarm event");
        let client_event = ClientSwarmEvent::from(event);
        client_event.handle_swarm_event(self);
    }

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(?msg, host = %self.handler.host, peer = %self.handler.swarm.local_peer_id(), "Handling core message");
        use FileShareCoreMessageHandler as FH;
        use NiketsuMessage::*;
        match msg {
            VideoStatus(msg) => ClientCoreMessageHandler::handle_core_message(msg, self),
            Select(msg) => ClientCoreMessageHandler::handle_core_message(msg, self),
            Playlist(msg) => ClientCoreMessageHandler::handle_core_message(msg, self),
            Status(msg) => ClientCoreMessageHandler::handle_core_message(msg, self),
            FileRequest(msg) => FH::handle_core_message(msg, &mut self.handler),
            FileResponse(msg) => FH::handle_core_message(msg, &mut self.handler),
            ChunkRequest(msg) => FH::handle_core_message(msg, &mut self.handler),
            ChunkResponse(msg) => FH::handle_core_message(msg, &mut self.handler),
            VideoShare(msg) => FH::handle_core_message(msg, &mut self.handler),
            msg => msg.broadcast(&mut self.handler),
        }
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!("Received swarm request {msg:?}");
        use FileShareSwarmRequestHandler as FH;
        use NiketsuMessage::*;
        match msg {
            FileRequest(msg) => FH::handle_swarm_request(msg, channel, &mut self.handler),
            ChunkRequest(msg) => FH::handle_swarm_request(msg, channel, &mut self.handler),
            msg if peer_id == self.handler.host => msg.send_to_core(channel, &mut self.handler),
            msg => msg.respond_with_err(channel, &mut self.handler),
        }
    }

    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        debug!(message = ?niketsu_msg, "Received broadcast");
        let swarm_broadcast = ClientSwarmBroadcast::from(niketsu_msg);
        swarm_broadcast.handle_swarm_broadcast(peer_id, self)
    }

    fn handler(&mut self) -> &mut CommunicationHandler {
        &mut self.handler
    }
}
