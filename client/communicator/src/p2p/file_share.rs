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

/// The local video server plays a file another peer shares.
#[derive(Debug)]
pub struct FileShareConsumer {
    /// The kademlia query for the file's providers.
    query: QueryId,
    /// The providers kademlia reported last. A new request for a file
    /// starts out with them, as its query may come back empty.
    providers: HashSet<PeerId>,
    stage: Stage,
    chunk_requests: HashMap<OutboundRequestId, oneshot::Sender<Vec<u8>>>,
    /// A chunk request failed and none has succeeded since. The user hears
    /// of a stall once, however many chunks fail in it.
    stalled: bool,
}

#[derive(Debug)]
enum Stage {
    /// Waiting for kademlia to name a provider for the request.
    Discovering(FileRequestMsg),
    /// The file request went to `provider`. Kept as `request` so a failure
    /// (typically a timeout after a failed hole punch) is reported to the
    /// core instead of leaving the consumer waiting forever.
    Requested {
        provider: PeerId,
        request: OutboundRequestId,
    },
    /// `provider` answered the file request.
    Streaming { provider: PeerId },
}

impl FileShareConsumer {
    fn new(query: QueryId, request: FileRequestMsg, providers: HashSet<PeerId>) -> Self {
        Self {
            query,
            providers,
            stage: Stage::Discovering(request),
            chunk_requests: HashMap::default(),
            stalled: false,
        }
    }

    /// Where chunks are fetched from, once the file was requested.
    fn provider(&self) -> Option<PeerId> {
        match self.stage {
            Stage::Discovering(_) => None,
            Stage::Requested { provider, .. } | Stage::Streaming { provider } => Some(provider),
        }
    }

    fn is_file_request(&self, id: OutboundRequestId) -> bool {
        matches!(self.stage, Stage::Requested { request, .. } if request == id)
    }

    /// Takes note that `id` was answered, and whether it was the file
    /// request.
    fn answered(&mut self, id: OutboundRequestId) -> bool {
        let Stage::Requested { provider, request } = self.stage else {
            return false;
        };
        if request != id {
            return false;
        }
        self.stage = Stage::Streaming { provider };
        true
    }

    fn handle_kad_event(&mut self, event: kad::Event, base: &mut CommonCommunication) {
        let kad::Event::OutboundQueryProgressed { id, result, .. } = &event else {
            return debug!(?event, "Received non handled kademlia event");
        };

        let kad::QueryResult::GetProviders(result) = result else {
            return debug!(?event, "Received non handled kademlia event");
        };

        if *id != self.query {
            return debug!(?result, "Ignoring the providers of an earlier file request");
        }

        match result {
            Ok(kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. }) => {
                debug!(?result, "Kademlia did not return new providers");
                self.request_file(base);
            }
            Ok(kad::GetProvidersOk::FoundProviders { providers, .. }) => {
                self.providers = providers.clone();
                self.request_file(base);
            }
            Err(_) if !matches!(self.stage, Stage::Discovering(_)) => {
                debug!(?result, "Kademlia query ended after the file was requested");
            }
            Err(_) => {
                debug!(?result, "No kademlia providers found");
                base.tell_user("No providers found for the requested file");
            }
        }
    }

    fn request_file(&mut self, base: &mut CommonCommunication) {
        let Stage::Discovering(request) = &self.stage else {
            debug!("File is already being shared. No need for further actions");
            return;
        };
        debug!("Requesting file from providers");

        // The request uuid is a random v4, so this picks a fresh random
        // provider per initiated file request: a retry after a failure is
        // not stuck with the same (possibly unreachable) peer.
        let index = request.uuid.as_u128() as usize % self.providers.len().max(1);
        let Some(provider) = self.providers.iter().nth(index).copied() else {
            debug!("No providers found for the requested file");
            base.tell_user("No providers found for the requested file");
            base.stream_stopped();
            return;
        };
        debug!(%provider, "Provider found");

        // A provider without a connection yet is dialed over the host's
        // circuit, the rendezvous for the hole punch.
        if provider != base.host && !base.swarm.is_connected(&provider) {
            match base.host_circuit() {
                Some(circuit) => {
                    if let Err(err) = base.swarm.dial(circuit.with(Protocol::P2p(provider))) {
                        error!(?err, "Failed to dial file provider via host relay");
                    }
                }
                None => error!("Host address unknown, cannot dial file provider via host relay"),
            }
        }

        let request = base
            .swarm
            .send_file_request(&provider, FileShareRequest::File(request.clone()));
        self.stage = Stage::Requested { provider, request };
    }
}

impl CommonCommunication {
    /// Tells the core that the stream its video server plays has ended.
    fn stream_stopped(&self) {
        self.message_sender
            .send(VideoProviderStoppedMsg.into())
            .ok();
    }
}

impl CommunicationHandler {
    /// Ends the file share of this peer, whichever side of it this peer is
    /// on, without a word to the user.
    pub(super) fn fs_end(&mut self) {
        if let Some(FileShare::Provider(provider)) = self.file_share.take() {
            self.base.swarm.stop_providing(provider.video());
        }
    }

    pub(super) fn fs_kad_event(&mut self, event: kad::Event) {
        if let Some(FileShare::Consumer(consumer)) = &mut self.file_share {
            consumer.handle_kad_event(event, &mut self.base);
        }
    }

    pub(super) fn fs_req_resp_event(
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
                    if !std::mem::replace(&mut consumer.stalled, true) {
                        self.base
                            .tell_user("Chunk download timeout. Connection might be slow...");
                    }
                } else if consumer.is_file_request(request_id) {
                    // Typically a timeout because the hole punch to the
                    // provider never produced a direct connection. Playback of
                    // this share has not started at this point, so reporting
                    // the stop cannot cut a running video short.
                    warn!(?error, "File request failed");
                    self.fs_stop_consuming("Requesting the file from the provider failed");
                }
            }
            _ => {}
        }
    }

    /// Ends the file share this peer consumes if `peer` provided it: with
    /// the link to the provider gone, no chunk request can be answered.
    pub(super) fn fs_peer_unlinked(&mut self, peer: PeerId) {
        let Some(FileShare::Consumer(consumer)) = &self.file_share else {
            return;
        };
        if consumer.provider() != Some(peer) {
            return;
        }
        warn!(%peer, "The provider of the file disconnected");
        self.fs_stop_consuming("The provider of the file disconnected");
    }

    /// Ends the file share this peer consumes and tells the user `why`. The
    /// chunk requests still pending go unanswered.
    fn fs_stop_consuming(&mut self, why: &str) {
        self.file_share.take();
        self.base.tell_user(why);
        self.base.stream_stopped();
    }

    pub(super) fn fs_video_share(&mut self, msg: VideoShareMsg) -> Result<()> {
        self.fs_end();
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
        let Some(FileShare::Consumer(consumer)) = &mut self.file_share else {
            self.base.stream_stopped();
            bail!("No active file share consumer");
        };
        let Some(provider) = consumer.provider() else {
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

        let providers = match self.file_share.take() {
            Some(FileShare::Consumer(c)) => c.providers,
            other => {
                self.file_share = other;
                HashSet::default()
            }
        };
        self.fs_end();

        let key = msg.video.as_str().as_bytes().to_vec().into();
        let query = self.base.swarm.kademlia().get_providers(key);
        let consumer = FileShareConsumer::new(query, msg, providers);
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
            is_file_request = consumer.answered(request_id);
            if reply.is_some() && resp.is_ok() {
                consumer.stalled = false;
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
                self.base.stream_stopped();
                bail!("{error}");
            }
        }
    }
}
