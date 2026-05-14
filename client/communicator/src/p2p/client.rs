use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::request_response::{self, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, gossipsub, mdns, ping};
use niketsu_core::communicator::{
    ConnectedMsg, PlaylistMsg, SeekMsg, SelectMsg, UserStatusMsg, VideoStatusMsg,
};
use niketsu_core::log_err_msg;
use tracing::{debug, error, info, trace, warn};

use super::{
    Behaviour, BehaviourEvent, CommunicationHandler, CommunicationHandlerTrait,
    FileShareBehaviourEvent, MessageResponse, MessagingBehaviourEvent, SwarmHandler,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

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

    fn on_ping(&mut self, event: ping::Event) {
        debug!("Received ping!");
        if event.peer != self.handler.host {
            return;
        }

        if let Some(conn) = self.host_conn
            && event.connection == conn
        {
            match event.result {
                Ok(d) => self.delay = d,
                Err(error) => warn!(%error, "Failed to get ping rtt"),
            }
        }
    }

    fn on_dcutr(&mut self, event: dcutr::Event) {
        match event.result {
            Ok(res) => {
                self.host_conn = Some(res);

                info!("Established direct connection. Closing connection to relay");
                if let Some(conn) = self.relay_conn {
                    self.handler.swarm.close_connection(conn);
                }

                let gossip = &mut self.handler.swarm.behaviour_mut().messaging.gossipsub;
                gossip.add_explicit_peer(&event.remote_peer_id);
            }
            Err(error) => error!(
                %event.remote_peer_id, %error,
                "Direct connection (hole punching) failed"
            ),
        }
    }

    fn on_gossipsub(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => {
                debug!(%message_id, %message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                let res = self.handle_swarm_broadcast(message.data, propagation_source);
                log_err_msg!(res, "Failed to handle broadcast message");
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }

    fn on_msg_req_resp(
        &mut self,
        event: request_response::Event<super::MessageRequest, MessageResponse>,
    ) {
        match event {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = self.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, "Received response");
                    let res = self.handler.handle_swarm_response(response, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
            },
            request_response::Event::OutboundFailure { peer, error, .. } => {
                let host = self.handler.host;
                if peer == self.handler.host {
                    warn!(%error, %peer, %host, "Outbound failure for request response");
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

    fn on_mdns(&mut self, event: mdns::Event) {
        if let mdns::Event::Discovered(list) = event {
            for (peer_id, addr) in list {
                self.handler
                    .swarm
                    .behaviour_mut()
                    .file_share
                    .kademlia
                    .add_address(&peer_id, addr);
            }
        }
    }

    fn on_connection_established(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        if peer_id != self.handler.host {
            return;
        }

        let gossip = &mut self.handler.swarm.behaviour_mut().messaging.gossipsub;
        gossip.add_explicit_peer(&peer_id);

        if endpoint.is_relayed() {
            self.relay_conn = Some(connection_id);
        } else {
            info!(%connection_id, ?endpoint, "Direct connection to host established!");
            self.handler
                .swarm
                .behaviour_mut()
                .file_share
                .kademlia
                .add_address(&peer_id, endpoint.get_remote_address().clone());
        }
        if let Err(error) = self.handler.message_sender.send(ConnectedMsg.into()) {
            warn!(%error, "Failed to send connected message to core");
        }
    }

    fn on_connection_closed(
        &mut self,
        peer_id: PeerId,
        cause: Option<ConnectionError>,
        connection_id: ConnectionId,
    ) {
        if peer_id == self.handler.host && !self.handler.swarm.is_connected(&peer_id) {
            warn!(?cause, ?peer_id, host = %self.handler.host, %connection_id, "Connection to host closed");
            self.handler.core_receiver.close();
        }
    }

    fn on_outgoing_connection_error(
        &mut self,
        connection_id: ConnectionId,
        peer_id: Option<PeerId>,
        error: DialError,
    ) {
        let Some(pid) = peer_id else {
            warn!(%error, "Outgoing connection error with unknown peer");
            return;
        };

        if let Some(conn) = self.host_conn
            && connection_id != conn
        {
            warn!(%error, %connection_id, "Outgoing connection error with non-host. Ignoring");
            return;
        }

        if pid == self.handler.host && !self.handler.swarm.is_connected(&pid) {
            warn!(?error, ?peer_id, host = %self.handler.host, %connection_id, "Connection error to host");
            self.handler.core_receiver.close();
        }
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!("Received swarm request {msg:?}");
        if peer_id == self.handler.host {
            self.handler.send_to_core(msg, channel)
        } else {
            self.handler.respond_with_err(msg, channel)
        }
    }

    fn handle_swarm_broadcast(&mut self, msg: Vec<u8>, peer_id: PeerId) -> Result<()> {
        let niketsu_msg: NiketsuMessage = msg.try_into()?;
        debug!(message = ?niketsu_msg, "Received broadcast");
        use NiketsuMessage::*;
        match niketsu_msg {
            VideoStatus(msg) => self.on_broadcast_video_status(msg, peer_id),
            Select(msg) => self.on_broadcast_select(msg),
            Seek(msg) => self.on_broadcast_seek(msg),
            other => {
                self.handler.message_sender.send(other)?;
                Ok(())
            }
        }
    }

    fn on_broadcast_video_status(&mut self, msg: VideoStatusMsg, peer_id: PeerId) -> Result<()> {
        if peer_id != self.handler.host {
            anyhow::bail!("Received video status from non-host peer: {peer_id:?}")
        }

        if self.is_seeking {
            debug!("can not determine client position during seek");
            return Ok(());
        }

        let mut video_status = msg;
        if let Some(pos) = video_status.position
            && !video_status.paused
        {
            debug!("add delay to position");
            video_status.position = Some(pos + self.delay.div_f64(2.0));
        }

        self.handler.message_sender.send(video_status.into())?;
        Ok(())
    }

    fn on_broadcast_select(&mut self, msg: SelectMsg) -> Result<()> {
        self.handler.reset_requests_responses();
        self.handler.message_sender.send(msg.into())?;
        Ok(())
    }

    fn on_broadcast_seek(&mut self, msg: SeekMsg) -> Result<()> {
        self.is_seeking = true;
        self.handler.message_sender.send(msg.into())?;
        Ok(())
    }

    fn on_core_user_status(&mut self, msg: UserStatusMsg) -> Result<()> {
        let host = self.handler.host;
        self.handler.swarm.send_request(&host, msg.into());
        Ok(())
    }

    fn on_core_playlist(&mut self, msg: PlaylistMsg) -> Result<()> {
        let host = self.handler.host;
        self.handler.swarm.send_request(&host, msg.into());
        Ok(())
    }

    fn on_core_video_status(&mut self, msg: VideoStatusMsg) -> Result<()> {
        if msg.position != self.video_status.position {
            self.is_seeking = false;
            self.video_status = msg;
        }
        Ok(())
    }

    fn on_core_select(&mut self, msg: SelectMsg) -> Result<()> {
        let topic = self.handler.topic.clone();
        self.handler.reset_requests_responses();
        self.handler.swarm.try_broadcast(topic, msg.into())
    }

    fn on_core_broadcast(&mut self, msg: NiketsuMessage) -> Result<()> {
        let topic = self.handler.topic.clone();
        self.handler.swarm.try_broadcast(topic, msg)
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
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        use MessagingBehaviourEvent as M;
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(Transport(T::Ping(e))) => self.on_ping(e),
            SwarmEvent::Behaviour(Transport(T::Dcutr(e))) => self.on_dcutr(e),
            SwarmEvent::Behaviour(Messaging(M::Gossipsub(e))) => self.on_gossipsub(e),
            SwarmEvent::Behaviour(Messaging(M::RequestResponse(e))) => self.on_msg_req_resp(e),
            SwarmEvent::Behaviour(FileShare(F::RequestResponse(e))) => {
                self.handler.handle_file_share_req_resp_event(e)
            }
            SwarmEvent::Behaviour(FileShare(F::Kademlia(e))) => {
                self.handler.handle_file_share_kad_event(e)
            }
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => self.on_connection_established(peer_id, connection_id, endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                connection_id,
                ..
            } => self.on_connection_closed(peer_id, cause, connection_id),
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            } => self.on_outgoing_connection_error(connection_id, peer_id, error),
            other => debug!(event = ?other, "Received not captured event"),
        }
    }

    fn handle_core_message(&mut self, msg: NiketsuMessage) -> Result<()> {
        debug!(?msg, host = %self.handler.host, peer = %self.handler.swarm.local_peer_id(), "Handling core message");
        use NiketsuMessage::*;
        match msg {
            VideoStatus(m) => self.on_core_video_status(m),
            Select(m) => self.on_core_select(m),
            Playlist(m) => self.on_core_playlist(m),
            Status(m) => self.on_core_user_status(m),
            m @ (FileRequest(_) | FileResponse(_) | ChunkRequest(_) | ChunkResponse(_)
            | VideoShare(_)) => self.handler.handle_file_share_core_message(m),
            other => self.on_core_broadcast(other),
        }
    }
}
