use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use arcstr::ArcStr;
use futures::FutureExt;
use futures::future::BoxFuture;
use libp2p::kad::QueryId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use libp2p::{PeerId, kad};
use niketsu_core::chunk::{ChunkRequest, ChunkRoute};
use niketsu_core::communicator::{
    FileRequestMsg, FileResponseMsg, VideoProviderStoppedMsg, VideoShareMsg,
};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::Video;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::{debug, error, warn};

use super::{CommonCommunication, CommunicationHandler};
use crate::p2p::SwarmHandler;

/// Chunks of the local video server requested from the provider and not yet
/// answered. The video server waits beyond that, so a slow provider slows it.
pub(super) const CHUNK_FETCHES: usize = 4;

/// The chunk routes of this peer, see [`niketsu_core::chunk`].
#[derive(Debug, Clone)]
pub(crate) struct ChunkRoutes {
    /// Requests of the local video server, which the communicator fetches
    /// from the provider.
    pub fetch: ChunkRoute,
    /// Requests of other peers, which the local video provider answers.
    pub provide: ChunkRoute,
}

/// A chunk the local video provider reads for another peer, answered on the
/// response channel. `None` means it cannot be served.
pub(super) type ChunkRead =
    BoxFuture<'static, (ResponseChannel<FileShareResponseResult>, Option<Vec<u8>>)>;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum FileShareRequest {
    File(FileRequestMsg),
    Chunk { video: ArcStr, start: u64, len: u64 },
}

pub(crate) type FileShareResponseResult = std::result::Result<FileShareResponse, String>;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum FileShareResponse {
    File(FileResponseMsg),
    Chunk(ChunkBytes),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ChunkBytes(Vec<u8>);

impl std::fmt::Debug for ChunkBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChunkBytes({} bytes)", self.0.len())
    }
}

#[derive(Debug)]
pub(crate) enum FileShare {
    Provider(FileShareProvider),
    Consumer(FileShareConsumer),
}

#[derive(Debug)]
pub struct FileShareProvider {
    pending_file_responses: HashMap<uuid::Uuid, ResponseChannel<FileShareResponseResult>>,
    current_response: Video,
}

impl FileShareProvider {
    pub fn new(video: Video) -> Self {
        Self {
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
    chunk_requests: HashMap<OutboundRequestId, oneshot::Sender<Vec<u8>>>,
    /// The in-flight file request, kept so a failure (typically a timeout
    /// after a failed hole punch) is reported to the core instead of leaving
    /// the consumer waiting forever.
    file_request: Option<OutboundRequestId>,
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

        let provider = self.request_providers.as_ref().and_then(|providers| {
            // The request uuid is a random v4, so this picks a fresh random
            // provider per initiated file request: a retry after a failure
            // is not stuck with the same (possibly unreachable) peer.
            let index = request.uuid.as_u128() as usize % providers.len().max(1);
            providers.iter().nth(index).copied()
        });

        let Some(p) = provider else {
            debug!("No providers found for the requested file");
            let msg = "No providers found for the requested file".into();
            base.send_chat_message(arcstr::literal!("server"), msg).ok();
            base.message_sender
                .send(VideoProviderStoppedMsg.into())
                .ok();
            return;
        };

        debug!("Provider found");
        self.current_request_provider = Some(p);

        // If the host is the provider we are already connected; otherwise
        // dial via the host's relay circuit so the host bridges to the peer.
        if p != base.host && !base.swarm.is_connected(&p) {
            match base.host_circuit() {
                Some(circuit) => {
                    if let Err(err) = base.swarm.dial(circuit.with(Protocol::P2p(p))) {
                        error!(?err, "Failed to dial file provider via host relay");
                    }
                }
                None => error!("Host address unknown, cannot dial file provider via host relay"),
            }
        }

        let request_id = base
            .swarm
            .send_file_request(&p, FileShareRequest::File(request.clone()));
        self.file_request = Some(request_id);
        self.is_requesting = true;
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
                    FileShareRequest::Chunk { video, start, len } => {
                        self.fs_swarm_chunk_request(video, start, len, channel).ok();
                    }
                },
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    self.fs_swarm_response(request_id, response).ok();
                }
            },
            request_response::Event::OutboundFailure {
                request_id, error, ..
            } => {
                let Some(FileShare::Consumer(consumer)) = &mut self.file_share else {
                    return;
                };
                if consumer.chunk_requests.remove(&request_id).is_some() {
                    warn!(?error, "Chunk request failed. Cache will retry.");
                    let msg = "Chunk download timeout. Connection might be slow...".to_string();
                    self.base
                        .send_chat_message(arcstr::literal!("server"), msg)
                        .ok();
                } else if consumer.file_request == Some(request_id) {
                    // Typically a timeout because the hole punch to the
                    // provider never produced a direct connection. Playback of
                    // this share has not started at this point, so reporting
                    // the stop cannot cut a running video short.
                    warn!(?error, "File request failed");
                    self.file_share.take();
                    let msg = "Requesting the file from the provider failed".to_string();
                    self.base
                        .send_chat_message(arcstr::literal!("server"), msg)
                        .ok();
                    self.base
                        .message_sender
                        .send(VideoProviderStoppedMsg.into())
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
        self.base.swarm.start_providing(&video)?;
        let provider = FileShareProvider::new(video);
        self.file_share = Some(FileShare::Provider(provider));
        Ok(())
    }

    /// Whether another chunk of the local video server may be requested from
    /// the provider, see [`CHUNK_FETCHES`].
    pub(super) fn can_fetch_chunk(&self) -> bool {
        match &self.file_share {
            Some(FileShare::Consumer(consumer)) => consumer.chunk_requests.len() < CHUNK_FETCHES,
            _ => true,
        }
    }

    pub(super) fn fs_fetch_chunk(&mut self, request: ChunkRequest) -> Result<()> {
        // TODO handle issues with provider
        let Some(FileShare::Consumer(consumer)) = &mut self.file_share else {
            self.base
                .message_sender
                .send(VideoProviderStoppedMsg.into())
                .ok();
            bail!("No active file share consumer");
        };
        let Some(provider) = consumer.current_request_provider else {
            bail!("No provider available for chunk request")
        };
        let ChunkRequest {
            file_name,
            start,
            len,
            reply,
        } = request;
        let msg = FileShareRequest::Chunk {
            video: file_name,
            start,
            len,
        };
        let request_id = self.base.swarm.send_file_request(&provider, msg);
        consumer.chunk_requests.insert(request_id, reply);
        Ok(())
    }

    pub(super) fn fs_chunk_read(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        bytes: Option<Vec<u8>>,
    ) -> Result<()> {
        let response = match bytes {
            Some(bytes) => Ok(FileShareResponse::Chunk(ChunkBytes(bytes))),
            None => Err(String::from("Not providing the requested file")),
        };
        self.base.swarm.send_file_response(channel, response)
    }

    pub(super) fn fs_file_request(&mut self, msg: FileRequestMsg) -> Result<()> {
        debug!(?msg.video, "Requesting file");

        let mut consumer = FileShareConsumer::default();
        if let Some(FileShare::Consumer(c)) = &self.file_share {
            consumer.request_providers = c.request_providers.clone();
        }
        self.reset_requests_responses();

        let key = msg.video.as_str().as_bytes().to_vec().into();
        let id = self.base.swarm.kademlia().get_providers(key);

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
        self.base.swarm.send_file_response(channel, resp)
    }

    /// Hands the request to the local video provider. When its queue is
    /// full, the request is dropped unanswered; the requester sees a failed
    /// request and its video server asks again later.
    fn fs_swarm_chunk_request(
        &mut self,
        file_name: ArcStr,
        start: u64,
        len: u64,
        channel: ResponseChannel<FileShareResponseResult>,
    ) -> Result<()> {
        let (reply, bytes) = oneshot::channel();
        let request = ChunkRequest {
            file_name,
            start,
            len,
            reply,
        };
        let sent = match self.base.provide.sender() {
            Some(sender) => sender.try_send(request),
            None => Err(TrySendError::Closed(request)),
        };
        match sent {
            Ok(()) => {
                let read = async move { (channel, bytes.await.ok()) };
                self.base.chunk_reads.push(read.boxed());
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                debug!("Video provider is busy, dropping chunk request");
                Ok(())
            }
            Err(TrySendError::Closed(_)) => {
                debug!("Got chunk request despite no active provider");
                let resp = Err(String::from("Not providing any files"));
                self.base.swarm.send_file_response(channel, resp)
            }
        }
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
        self.base.message_sender.send(msg.into())?;
        Ok(())
    }

    fn fs_swarm_response(
        &mut self,
        request_id: OutboundRequestId,
        resp: FileShareResponseResult,
    ) -> Result<()> {
        let mut reply = None;
        let mut is_file_request = false;
        if let Some(FileShare::Consumer(consumer)) = &mut self.file_share {
            reply = consumer.chunk_requests.remove(&request_id);
            is_file_request = consumer.file_request == Some(request_id);
            if is_file_request {
                consumer.file_request = None;
            }
        }
        match resp {
            Ok(FileShareResponse::Chunk(ChunkBytes(bytes))) => {
                // a request no longer pending has nobody waiting for it
                if let Some(reply) = reply {
                    reply.send(bytes).ok();
                }
                Ok(())
            }
            Ok(FileShareResponse::File(msg)) => {
                self.base.message_sender.send(msg.into())?;
                Ok(())
            }
            // the answer to a request of an earlier file share says nothing
            // about the current one
            Err(error) if reply.is_none() && !is_file_request => {
                debug!(%error, "Ignoring the failure of a request no longer pending");
                Ok(())
            }
            Err(error) => {
                self.file_share.take();
                self.base
                    .message_sender
                    .send(VideoProviderStoppedMsg.into())
                    .ok();
                bail!("{error}");
            }
        }
    }
}
