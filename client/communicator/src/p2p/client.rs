use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::kad::{self};
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{PeerId, dcutr, gossipsub, ping};
use niketsu_core::communicator::{
    ChunkRequestMsg, ChunkResponseMsg, ConnectedMsg, FileRequestMsg, FileResponseMsg, PlaylistMsg,
    SeekMsg, SelectMsg, UserStatusMsg, VideoShareMsg, VideoStatusMsg,
};
use tracing::{debug, error, info, trace, warn};

use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait, MessageResponse,
    Response, StatusResponse, SwarmEventHandler, SwarmHandler,
};
use crate::messages::NiketsuMessage;
use crate::p2p::MessageRequest;

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
                handler
                    .handler
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&self.remote_peer_id);
            }
            Err(error) => {
                error!(%self.remote_peer_id, %error, "Direct connection (hole punching) failed");
            }
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
                if let Err(error) = handler.handle_swarm_broadcast(message.data, propagation_source)
                {
                    error!(%error, "Failed to handle broadcast message");
                }
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
                    if let Err(error) = handler.handle_swarm_request(req, channel, peer) {
                        error!(%error, "Failed to handle incoming message");
                    }
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    if let Err(error) = handler.handle_swarm_response(response, peer) {
                        error!(%error, "Failed to handle incoming message");
                    }
                }
            },
            request_response::Event::OutboundFailure { peer, error, .. } => {
                let host = handler.handler.host;
                if peer == handler.handler.host {
                    warn!(
                        "Outbound failure for request response with peer: error: {error:?} from {peer:?} where host {host:?}"
                    );
                    // self.core_receiver.close();
                }
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
        SwarmEventHandler::handle_swarm_event(self, &mut handler.handler);
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

        if self.endpoint.is_relayed() {
            handler.relay_conn = Some(self.connection_id);
        }

        info!(%self.connection_id, ?self.endpoint, "Connection to host established!");
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

        if pid == handler.handler.host {
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

#[enum_dispatch(ClientCoreMessageHandler)]
enum ClientCoreMessage {
    VideoStatus(VideoStatusMsg),
    Playlist(PlaylistMsg),
    UserStatus(UserStatusMsg),
    Select(SelectMsg),
    VideoShare(VideoShareMsg),
    ChunkRequest(ChunkRequestMsg),
    ChunkResponse(ChunkResponseMsg),
    FileRequest(FileRequestMsg),
    FileResponse(FileResponseMsg),
    Other(NiketsuMessage),
}

impl ClientCoreMessage {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::VideoStatus(msg) => ClientCoreMessage::VideoStatus(msg),
            NiketsuMessage::Select(msg) => ClientCoreMessage::Select(msg),
            NiketsuMessage::Playlist(msg) => ClientCoreMessage::Playlist(msg),
            NiketsuMessage::Status(msg) => ClientCoreMessage::UserStatus(msg),
            NiketsuMessage::FileRequest(msg) => ClientCoreMessage::FileRequest(msg),
            NiketsuMessage::FileResponse(msg) => ClientCoreMessage::FileResponse(msg),
            NiketsuMessage::ChunkRequest(msg) => ClientCoreMessage::ChunkRequest(msg),
            NiketsuMessage::ChunkResponse(msg) => ClientCoreMessage::ChunkResponse(msg),
            NiketsuMessage::VideoShare(msg) => ClientCoreMessage::VideoShare(msg),
            msg => ClientCoreMessage::Other(msg),
        }
    }
}

impl ClientCoreMessageHandler for UserStatusMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        handler
            .handler
            .swarm
            .send_request(&handler.handler.host, self.into());
        Ok(())
    }
}

impl ClientCoreMessageHandler for PlaylistMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        handler
            .handler
            .swarm
            .send_request(&handler.handler.host, self.into());
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
        handler.reset_requests_responses();
        handler
            .handler
            .swarm
            .try_broadcast(handler.handler.topic.clone(), self.into())
    }
}

impl ClientCoreMessageHandler for VideoShareMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        match &self.video {
            Some(video) => {
                handler.handler.current_response = Some(video.clone());
                handler.handler.swarm.start_providing(video.clone())
            }
            None => {
                handler.reset_requests_responses();
                Ok(())
            }
        }
    }
}
impl ClientCoreMessageHandler for ChunkRequestMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        //TODO handle issues with provider
        match handler.handler.pending_request_provider {
            Some(provider) => {
                handler.handler.swarm.send_request(&provider, self.into());
                Ok(())
            }
            None => bail!("No provider available for chunk request"),
        }
    }
}

impl ClientCoreMessageHandler for ChunkResponseMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        if let Some(channel) = handler.handler.pending_chunk_responses.remove(&self.uuid) {
            let msg = NiketsuMessage::ChunkResponse(self);
            return handler
                .handler
                .swarm
                .send_response(channel, MessageResponse(Response::Message(msg)));
        }
        bail!("No access to response channel for chunk response");
    }
}

impl ClientCoreMessageHandler for FileRequestMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let id = handler
            .handler
            .swarm
            .behaviour_mut()
            .kademlia
            .get_providers(self.video.as_str().as_bytes().to_vec().into());
        debug!(?id, "Getting providers for file ...");
        handler.handler.current_requests.insert(id, self.clone());
        Ok(())
    }
}

impl ClientCoreMessageHandler for FileResponseMsg {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        let Some(channel) = handler.handler.pending_file_responses.remove(&self.uuid) else {
            bail!("Cannot send file response if response channel does not exist");
        };

        if self.video.is_none() {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::NotProvidingErr)),
            )
        } else {
            handler
                .handler
                .swarm
                .send_response(channel, MessageResponse(Response::Message(self.into())))
        }
    }
}

impl ClientCoreMessageHandler for NiketsuMessage {
    fn handle_core_message(self, handler: &mut ClientCommunicationHandler) -> Result<()> {
        handler
            .handler
            .swarm
            .try_broadcast(handler.handler.topic.clone(), self)
    }
}

#[enum_dispatch]
pub(crate) trait ClientSwarmRequestHandler {
    fn handle_swarm_client_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()>;
    fn handle_swarm_host_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        self.handle_swarm_client_request(channel, handler)
    }
    fn handle_swarm_request(
        &self,
        peer_id: PeerId,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        if peer_id == handler.handler.host {
            self.handle_swarm_host_request(channel, handler)
        } else {
            self.handle_swarm_client_request(channel, handler)
        }
    }
}

#[enum_dispatch(ClientSwarmRequestHandler)]
enum ClientSwarmRequest {
    ChunkRequest(ChunkRequestMsg),
    FileRequest(FileRequestMsg),
    Other(NiketsuMessage),
}

impl ClientSwarmRequest {
    fn from(message: NiketsuMessage) -> Self {
        match message {
            NiketsuMessage::FileRequest(msg) => ClientSwarmRequest::FileRequest(msg),
            NiketsuMessage::ChunkRequest(msg) => ClientSwarmRequest::ChunkRequest(msg),
            msg => ClientSwarmRequest::Other(msg),
        }
    }
}

impl ClientSwarmRequestHandler for ChunkRequestMsg {
    fn handle_swarm_client_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler
            .handler
            .pending_chunk_responses
            .insert(self.uuid, channel);
        handler
            .handler
            .message_sender
            .send(self.clone().into())
            .map_err(anyhow::Error::from)
    }
}

impl ClientSwarmRequestHandler for FileRequestMsg {
    fn handle_swarm_client_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler
            .handler
            .pending_file_responses
            .insert(self.uuid, channel);
        handler
            .handler
            .message_sender
            .send(self.clone().into())
            .map_err(anyhow::Error::from)
    }
}

impl ClientSwarmRequestHandler for NiketsuMessage {
    fn handle_swarm_client_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        handler.handler.swarm.send_response(
            channel,
            MessageResponse(Response::Status(StatusResponse::Err)),
        )?;
        bail!("Did not expect direct message from client {self:?}");
    }

    fn handle_swarm_host_request(
        &self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut ClientCommunicationHandler,
    ) -> Result<()> {
        if let (NiketsuMessage::FileResponse(_), NiketsuMessage::ChunkResponse(_)) = (&self, &self)
        {
            handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            )?;
            bail!("Did not expect direct message for responses {self:?}");
        }

        // typically, any host message is processed by the core
        match handler.handler.message_sender.send(self.clone()) {
            Ok(_) => handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Ok)),
            ),
            Err(_) => handler.handler.swarm.send_response(
                channel,
                MessageResponse(Response::Status(StatusResponse::Err)),
            ),
        }
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
        handler.reset_requests_responses();
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
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<NiketsuMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<NiketsuMessage>,
    ) -> Self {
        let handler = CommunicationHandler::new(swarm, topic, host, core_receiver, message_sender);
        Self {
            handler,
            host_conn: None,
            relay_conn: None,
            video_status: VideoStatusMsg::default(),
            is_seeking: false,
            delay: Duration::default(),
        }
    }

    fn reset_requests_responses(&mut self) {
        if let Some(video) = self.handler.current_response.clone() {
            self.handler.swarm.stop_providing(video);
        }
        self.handler.pending_chunk_responses = Default::default();
        self.handler.current_response = None;
        self.handler.current_requests = Default::default();
        self.handler.pending_request_provider = None;
    }
}

#[async_trait]
impl CommunicationHandlerTrait for ClientCommunicationHandler {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                event = self.handler.swarm.select_next_some() => self.handle_swarm_event(event),
                msg = self.handler.core_receiver.recv() => match msg {
                    Some(msg) => {
                        debug!(?msg, "Message from core");
                        if let Err(error) = self.handle_core_message(msg) {
                            error!(%error, "Handling message caused error");
                        }
                    },
                    None => {
                        debug!("Channel of core closed. Stopping p2p client event loop");
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
        let core_message = ClientCoreMessage::from(msg);
        core_message.handle_core_message(self)
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!("Received swarm request {msg:?}");
        let swarm_request = ClientSwarmRequest::from(msg);
        swarm_request.handle_swarm_request(peer_id, channel, self)
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
