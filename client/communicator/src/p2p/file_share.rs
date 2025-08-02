use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use libp2p::kad::QueryId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use libp2p::{PeerId, kad};
use niketsu_core::communicator::{
    ChunkRequestMsg, ChunkResponseMsg, FileRequestMsg, FileResponseMsg, VideoShareMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::Video;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

use super::{CommonCommunication, CommunicationHandler};
use crate::NiketsuMessage;
use crate::p2p::SwarmHandler;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum FileShareRequest {
    File(FileRequestMsg),
    Chunk(ChunkRequestMsg),
}

pub(crate) type FileShareResponseResult = std::result::Result<FileShareResponse, String>;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum FileShareResponse {
    File(FileResponseMsg),
    Chunk(ChunkResponseMsg),
}

#[derive(Debug)]
pub(crate) enum FileShare {
    Provider(FileShareProvider),
    Consumer(FileShareConsumer),
}

#[derive(Debug)]
pub struct FileShareProvider {
    pending_chunk_responses: HashMap<uuid::Uuid, ResponseChannel<FileShareResponseResult>>,
    pending_file_responses: HashMap<uuid::Uuid, ResponseChannel<FileShareResponseResult>>,
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
    file_requests: HashMap<QueryId, FileRequestMsg>,
    chunk_requests: HashSet<OutboundRequestId>,
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

        let Some(request) = self.file_requests.get(id) else {
            warn!("Found providers but no request?");
            return;
        };

        if let Some(provider) = &self.request_providers {
            if let Some(p) = provider.iter().next() {
                debug!("Provider found");
                self.current_request_provider = Some(*p);

                if !base.swarm.is_connected(p) {
                    let relayed_peer = base
                        .relay_addr
                        .clone()
                        .with(Protocol::P2pCircuit)
                        .with(Protocol::P2p(*p));
                    if let Err(err) = base.swarm.dial(relayed_peer) {
                        error!(?err, "Failed to dial file provider");
                    }
                }

                let req = FileShareRequest::File(request.clone());
                base.swarm.send_file_request(p, req);
                self.is_requesting = true;
            }
        }
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

impl FileShareEventHandler for request_response::Event<FileShareRequest, FileShareResponseResult> {
    fn handle_event(self, handler: &mut CommunicationHandler) {
        use request_response::Message::*;
        match self {
            request_response::Event::Message { message, .. } => match message {
                Request {
                    request, channel, ..
                } => match request {
                    FileShareRequest::File(msg) => {
                        FileShareSwarmRequestHandler::handle_swarm_request(msg, channel, handler)
                            .ok();
                    }
                    FileShareRequest::Chunk(msg) => {
                        FileShareSwarmRequestHandler::handle_swarm_request(msg, channel, handler)
                            .ok();
                    }
                },
                Response { response, .. } => {
                    FileShareSwarmResponseHandler::handle_swarm_response(response, handler).ok();
                }
            },
            request_response::Event::OutboundFailure { request_id, .. } => {
                if let Some(FileShare::Consumer(consumer)) = &mut handler.file_share {
                    if consumer.chunk_requests.contains(&request_id) {
                        handler.file_share.take();
                        let msg = NiketsuMessage::VideoProviderStopped(Default::default());
                        handler.message_sender.send(msg).unwrap();
                    }
                };
            }
            _ => {}
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
        let msg = Ok(FileShareResponse::Chunk(self));
        handler.swarm.send_file_response(channel, msg)
    }
}

impl FileShareCoreMessageHandler for ChunkRequestMsg {
    fn handle_core_message(self, handler: &mut CommunicationHandler) -> Result<()> {
        // TODO handle issues with provider
        let Some(FileShare::Consumer(consumer)) = &mut handler.file_share else {
            let msg = NiketsuMessage::VideoProviderStopped(Default::default());
            handler.message_sender.send(msg).unwrap();
            bail!("No active file share consumer");
        };
        let Some(provider) = consumer.current_request_provider else {
            bail!("No provider available for chunk request")
        };
        let request_id = handler
            .base
            .swarm
            .send_file_request(&provider, FileShareRequest::Chunk(self));
        consumer.chunk_requests.insert(request_id);
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

        consumer.file_requests.insert(id, self.clone());
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
            Err(String::from("Not providing any files"))
        } else {
            Ok(FileShareResponse::File(self))
        };
        handler.swarm.send_file_response(channel, resp)
    }
}

pub trait FileShareSwarmRequestHandler {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<FileShareResponseResult>,
        handler: &mut CommunicationHandler,
    ) -> Result<()>;
}

impl FileShareSwarmRequestHandler for ChunkRequestMsg {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<FileShareResponseResult>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            debug!("Got chunk request despite no active provider");
            let resp = Err(String::from("Not providing any files"));
            return handler.base.swarm.send_file_response(channel, resp);
        };
        provider.pending_chunk_responses.insert(self.uuid, channel);
        handler.message_sender.send(self.clone().into())?;
        Ok(())
    }
}

impl FileShareSwarmRequestHandler for FileRequestMsg {
    fn handle_swarm_request(
        self,
        channel: ResponseChannel<FileShareResponseResult>,
        handler: &mut CommunicationHandler,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut handler.file_share else {
            warn!(msg = ?self, "Got file request despite no active provider");
            let resp = Err(String::from("Not providing any files"));
            return handler.base.swarm.send_file_response(channel, resp);
        };
        provider.pending_file_responses.insert(self.uuid, channel);
        handler.message_sender.send(self.clone().into())?;
        Ok(())
    }
}

pub trait FileShareSwarmResponseHandler {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()>;
}

impl FileShareSwarmResponseHandler for FileShareResponseResult {
    fn handle_swarm_response(self, handler: &mut CommunicationHandler) -> Result<()> {
        let Ok(resp) = self else {
            handler.file_share.take();
            let msg = NiketsuMessage::VideoProviderStopped(Default::default());
            handler.message_sender.send(msg).unwrap();
            bail!("{}", self.unwrap_err());
        };

        handler.message_sender.send(resp.into())?;
        Ok(())
    }
}

impl From<FileShareResponse> for NiketsuMessage {
    fn from(value: FileShareResponse) -> Self {
        match value {
            FileShareResponse::File(file_response_msg) => file_response_msg.into(),
            FileShareResponse::Chunk(chunk_response_msg) => chunk_response_msg.into(),
        }
    }
}
