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

        if let Some(provider) = &self.request_providers
            && let Some(p) = provider.iter().next()
        {
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
        } else {
            debug!("No providers found for the requested file");
            let msg = "No providers found for the requested file".into();
            base.send_chat_message(arcstr::literal!("server"), msg).ok();
            let msg = NiketsuMessage::VideoProviderStopped(Default::default());
            base.message_sender.send(msg).ok();
        }
    }
}

impl CommunicationHandler {
    pub(super) fn handle_file_share_kad_event(&mut self, event: kad::Event) {
        if let Some(FileShare::Consumer(consumer)) = &mut self.file_share {
            consumer.handle_kad_event(event, &mut self.base);
        }
    }

    pub(super) fn handle_file_share_req_resp_event(
        &mut self,
        event: request_response::Event<FileShareRequest, FileShareResponseResult>,
    ) {
        match event {
            request_response::Event::Message { message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => match request {
                    FileShareRequest::File(msg) => {
                        self.fs_swarm_file_request(msg, channel).ok();
                    }
                    FileShareRequest::Chunk(msg) => {
                        self.fs_swarm_chunk_request(msg, channel).ok();
                    }
                },
                request_response::Message::Response { response, .. } => {
                    self.fs_swarm_response(response).ok();
                }
            },
            request_response::Event::OutboundFailure {
                request_id, error, ..
            } => {
                if let Some(FileShare::Consumer(consumer)) = &mut self.file_share
                    && consumer.chunk_requests.remove(&request_id)
                {
                    warn!(?error, "Chunk request failed. Cache will retry.");
                    let msg = "Chunk download timeout. Connection might be slow...".to_string();
                    self.base
                        .send_chat_message(arcstr::literal!("server"), msg)
                        .ok();
                }
            }
            _ => {}
        }
    }

    pub(super) fn fs_video_share(&mut self, msg: VideoShareMsg) -> Result<()> {
        self.reset_requests_responses();
        let Some(video) = msg.video else {
            return Ok(());
        };
        self.swarm.start_providing(&video)?;
        let provider = FileShareProvider::new(video);
        self.file_share = Some(FileShare::Provider(provider));
        Ok(())
    }

    pub(super) fn fs_chunk_response(&mut self, msg: ChunkResponseMsg) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut self.file_share else {
            bail!("No active file share provider");
        };
        let Some(channel) = provider.pending_chunk_responses.remove(&msg.uuid) else {
            bail!("No access to response channel for chunk response");
        };
        let response = Ok(FileShareResponse::Chunk(msg));
        self.swarm.send_file_response(channel, response)
    }

    pub(super) fn fs_chunk_request(&mut self, msg: ChunkRequestMsg) -> Result<()> {
        // TODO handle issues with provider
        let Some(FileShare::Consumer(consumer)) = &mut self.file_share else {
            let stopped = NiketsuMessage::VideoProviderStopped(Default::default());
            self.message_sender.send(stopped).unwrap();
            bail!("No active file share consumer");
        };
        let Some(provider) = consumer.current_request_provider else {
            bail!("No provider available for chunk request")
        };
        let request_id = self
            .base
            .swarm
            .send_file_request(&provider, FileShareRequest::Chunk(msg));
        if let Some(FileShare::Consumer(consumer)) = &mut self.file_share {
            consumer.chunk_requests.insert(request_id);
        }
        Ok(())
    }

    pub(super) fn fs_file_request(&mut self, msg: FileRequestMsg) -> Result<()> {
        debug!(?msg.video, "Requesting file");

        let mut consumer = FileShareConsumer::default();
        if let Some(FileShare::Consumer(c)) = &self.file_share {
            consumer.request_providers = c.request_providers.clone();
        }
        self.reset_requests_responses();

        let kademlia = &mut self.base.swarm.behaviour_mut().file_share.kademlia;
        let id = kademlia.get_providers(msg.video.as_str().as_bytes().to_vec().into());

        consumer.file_requests.insert(id, msg);
        self.file_share = Some(FileShare::Consumer(consumer));
        Ok(())
    }

    pub(super) fn fs_file_response(&mut self, msg: FileResponseMsg) -> Result<()> {
        debug!(?msg, "Responding to file request ...");
        let Some(FileShare::Provider(provider)) = &mut self.file_share else {
            bail!("No active file share provider");
        };
        let Some(channel) = provider.pending_file_responses.remove(&msg.uuid) else {
            bail!("Cannot send file response if response channel does not exist");
        };

        let resp = if msg.video.is_none() {
            Err(String::from("Not providing any files"))
        } else {
            Ok(FileShareResponse::File(msg))
        };
        self.swarm.send_file_response(channel, resp)
    }

    fn fs_swarm_chunk_request(
        &mut self,
        msg: ChunkRequestMsg,
        channel: ResponseChannel<FileShareResponseResult>,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut self.file_share else {
            debug!("Got chunk request despite no active provider");
            let resp = Err(String::from("Not providing any files"));
            return self.base.swarm.send_file_response(channel, resp);
        };
        provider.pending_chunk_responses.insert(msg.uuid, channel);
        self.message_sender.send(msg.into())?;
        Ok(())
    }

    fn fs_swarm_file_request(
        &mut self,
        msg: FileRequestMsg,
        channel: ResponseChannel<FileShareResponseResult>,
    ) -> Result<()> {
        let Some(FileShare::Provider(provider)) = &mut self.file_share else {
            warn!(?msg, "Got file request despite no active provider");
            let resp = Err(String::from("Not providing any files"));
            return self.base.swarm.send_file_response(channel, resp);
        };
        provider.pending_file_responses.insert(msg.uuid, channel);
        self.message_sender.send(msg.into())?;
        Ok(())
    }

    fn fs_swarm_response(&mut self, resp: FileShareResponseResult) -> Result<()> {
        let Ok(r) = resp else {
            self.file_share.take();
            let msg = NiketsuMessage::VideoProviderStopped(Default::default());
            self.message_sender.send(msg).unwrap();
            bail!("{}", resp.unwrap_err());
        };

        self.message_sender.send(r.into())?;
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
