use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use libp2p::kad::QueryId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use libp2p::{PeerId, kad};
use niketsu_core::communicator::{
    ChunkRequestMsg, ChunkResponseMsg, FileRequestMsg, FileResponseMsg, VideoShareMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::Video;
use tracing::{debug, error, warn};

use super::{CommonCommunication, CommunicationHandler, MessageResponse};
use crate::NiketsuMessage;
use crate::p2p::{MessageRequest, Response, StatusResponse, SwarmHandler};

#[derive(Debug)]
pub(crate) enum FileShare {
    Provider(FileShareProvider),
    Consumer(FileShareConsumer),
}

#[derive(Debug)]
pub struct FileShareProvider {
    pending_chunk_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    pending_file_responses: HashMap<uuid::Uuid, ResponseChannel<MessageResponse>>,
    current_response: Video,
}

impl FileShareProvider {
    pub fn new(video: Video) -> Self {
        Self {
            pending_chunk_responses: Default::default(),
            pending_file_responses: Default::default(),
            current_response: video,
        }
    }

    pub fn video(&self) -> &Video {
        &self.current_response
    }
}

#[derive(Debug, Default)]
pub struct FileShareConsumer {
    current_requests: HashMap<QueryId, FileRequestMsg>,
    current_request_provider: Option<PeerId>,
    request_providers: Option<HashSet<PeerId>>,
    is_requesting: bool,
}

impl FileShareConsumer {
    fn handle_kad_event(&mut self, event: kad::Event, base: &mut CommonCommunication) {
        let kad::Event::OutboundQueryProgressed { id, result, .. } = &event else {
            return debug!(?event, "Received non handled kademlia event");
        };

        let kad::QueryResult::GetProviders(result) = result else {
            return debug!(?event, "Received non handled kademlia event");
        };

        if let Ok(kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. }) = result {
            debug!(?result, "Kademlia did not return new providers");
            self.request_file(id, base);
            return;
        };

        let Ok(kad::GetProvidersOk::FoundProviders { providers, .. }) = result else {
            debug!(?result, "No kademlia providers found");
            let msg = "No providers found for the requested file".into();
            let res = base.send_chat_message(arcstr::literal!("server"), msg);
            return log_err_msg!(res, "Failed to send message to core");
        };

        self.request_providers = Some(providers.clone());
        self.request_file(id, base);
    }

    fn request_file(&mut self, id: &QueryId, base: &mut CommonCommunication) {
        debug!("Requesting file from providers");

        if self.is_requesting {
            debug!("File is already being shared. No need for further actions");
            return;
        }

        let Some(request) = self.current_requests.get(id) else {
            warn!("Found providers but no request?");
            return;
        };

        if let Some(provider) = &self.request_providers {
            if let Some(p) = provider.iter().next() {
                debug!("Provider found");
                self.current_request_provider = Some(*p);

                let connect = base.swarm.is_connected(p);
                println!("Is connected: {connect:?}, {provider:?}");

                if !base.swarm.is_connected(p) {
                    let relayed_peer = base
                        .relay_addr
                        .clone()
                        .with(Protocol::P2pCircuit)
                        .with(Protocol::P2p(base.host));
                    if let Err(err) = base.swarm.dial(relayed_peer) {
                        error!(?err, "Failed to dial file provider");
                    }
                }

                base.swarm
                    .behaviour_mut()
                    .message_request_response
                    .send_request(
                        p,
                        MessageRequest(NiketsuMessage::FileRequest(request.clone())),
                    );
                self.is_requesting = true;
            }
        }
        println!("NO PROCVIDERS");
    }
}

pub trait FileShareEventHandler {
    fn handle_event(self, handler: &mut CommunicationHandler);
}

impl FileShareEventHandler for kad::Event {
    fn handle_event(self, handler: &mut CommunicationHandler) {
        if let Some(FileShare::Consumer(consumer)) = &mut handler.file_share {
            consumer.handle_kad_event(self, &mut handler.base)
        }
    }
}

pub trait FileShareCoreMessageHandler {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()>;
}

impl FileShareCoreMessageHandler for VideoShareMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        handler.reset_requests_responses();
        let Some(video) = self.video else {
            return Ok(());
        };
        handler.swarm.start_providing(&video)?;
        let provider = FileShareProvider::new(video);
        handler.file_share = Some(FileShare::Provider(provider));
        Ok(())
    }
}

impl FileShareCoreMessageHandler for ChunkResponseMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            bail!("No active file share provider");
        };
        let Some(channel) = provider.pending_chunk_responses.remove(&self.uuid) else {
            bail!("No access to response channel for chunk response");
        };
        let msg = Response::Message(NiketsuMessage::ChunkResponse(self));
        handler.swarm.send_response(channel, MessageResponse(msg))
    }
}

impl FileShareCoreMessageHandler for ChunkRequestMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        // TODO handle issues with provider
        let Some(FileShare::Consumer(consumer)) = &mut handler.file_share else {
            bail!("No active file share consumer");
        };
        let Some(provider) = consumer.current_request_provider else {
            bail!("No provider available for chunk request")
        };
        handler.swarm.send_request(&provider, self.into());
        Ok(())
    }
}

impl FileShareCoreMessageHandler for FileRequestMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        debug!(?self.video, "Requesting file");

        let mut consumer = FileShareConsumer::default();
        if let Some(FileShare::Consumer(c)) = &handler.file_share {
            consumer.request_providers = c.request_providers.clone();
        }
        handler.reset_requests_responses();

        let kademlia = &mut handler.base.swarm.behaviour_mut().kademlia;
        let id = kademlia.get_providers(self.video.as_str().as_bytes().to_vec().into());

        consumer.current_requests.insert(id, self.clone());
        handler.file_share = Some(FileShare::Consumer(consumer));
        Ok(())
    }
}

impl FileShareCoreMessageHandler for FileResponseMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        debug!(?self, "Responding to file request ...");
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            bail!("No active file share provider");
        };
        let Some(channel) = provider.pending_file_responses.remove(&self.uuid) else {
            bail!("Cannot send file response if response channel does not exist");
        };

        let resp = if self.video.is_none() {
            Response::Status(StatusResponse::NotProvidingErr)
        } else {
            Response::Message(self.into())
        };
        handler.swarm.send_response(channel, MessageResponse(resp))
    }
}

pub trait FileShareSwarmRequestHandler {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut CommunicationHandler,
    ) -> Result<()>;
}

impl FileShareSwarmRequestHandler for ChunkRequestMsg {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            debug!("Got chunk request despite no active provider");
            let resp = MessageResponse(Response::Status(StatusResponse::NotProvidingErr));
            return handler.base.swarm.send_response(channel, resp);
        };
        provider.pending_chunk_responses.insert(self.uuid, channel);
        handler.message_sender.send(self.clone().into())?;
        Ok(())
    }
}

impl FileShareSwarmRequestHandler for FileRequestMsg {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<MessageResponse>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            warn!(msg = ?self, "Got file request despite no active provider");
            let resp = MessageResponse(Response::Status(StatusResponse::NotProvidingErr));
            return handler.base.swarm.send_response(channel, resp);
        };
        provider.pending_file_responses.insert(self.uuid, channel);
        handler.message_sender.send(self.clone().into())?;
        Ok(())
    }
}

pub trait FileShareSwarmResponseHandler {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()>;
}

impl FileShareSwarmResponseHandler for StatusResponse {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        debug!(?self, "Received status response");
        if self == StatusResponse::NotProvidingErr {
            let Some(FileShare::Consumer(consumer)) = &mut handler.file_share else {
                bail!("No active file share consumer");
            };
            // FIXME: this might not work since all potential providers are lost
            consumer.current_request_provider.take();
        }
        Ok(())
    }
}

impl FileShareSwarmResponseHandler for ChunkResponseMsg {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        handler.message_sender.send(self.into())?;
        Ok(())
    }
}

impl FileShareSwarmResponseHandler for FileResponseMsg {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        handler.message_sender.send(self.into())?;
        Ok(())
    }
}
