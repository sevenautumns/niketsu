use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, bail};
use arcstr::ArcStr;
use futures::FutureExt;
use futures::future::BoxFuture;
use libp2p::PeerId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use niketsu_core::chunk::{ChunkRequest, ChunkRoute, ProvideRoute, Server, SharedFile};
use niketsu_core::communicator::{StreamEndedMsg, StreamMsg};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::{debug, error, warn};

use super::member::Tagged;
use super::{Net, Session, SwarmExt};

/// Chunks of the local video server requested from the provider and not yet
/// answered. The video server waits beyond that, so a slow provider slows it.
pub(super) const CHUNK_FETCHES: usize = 4;

/// The chunk routes of this peer, see [`niketsu_core::chunk`].
#[derive(Debug, Clone)]
pub(crate) struct ChunkRoutes {
    /// Requests of the local video server, which the communicator fetches
    /// from the provider.
    pub fetch: ChunkRoute,
    /// The file this peer offers, and where the chunk requests of other
    /// peers for it go: to the local video provider.
    pub provide: ProvideRoute,
}

/// A chunk the local video provider reads for another peer, answered on the
/// response channel. `None` means it cannot be served.
pub(super) type ChunkRead =
    BoxFuture<'static, (ResponseChannel<FileShareResponseResult>, Option<Vec<u8>>)>;

/// A byte range of the file of `video`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FileShareRequest {
    pub video: ArcStr,
    pub start: u64,
    pub len: u64,
}

/// The chunk, or why the provider does not serve it.
pub(crate) type FileShareResponseResult = std::result::Result<ChunkBytes, String>;

/// Goes on the wire as one CBOR byte string; as a sequence of integers a
/// chunk would take about twice its size.
#[derive(Serialize, Deserialize)]
pub(crate) struct ChunkBytes(#[serde(with = "serde_bytes")] pub(super) Vec<u8>);

impl std::fmt::Debug for ChunkBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChunkBytes({} bytes)", self.0.len())
    }
}

/// Who serves the requests for `video`, or why nobody does: this peer
/// serves what its provide route serves.
fn server_of(provide: &ProvideRoute, video: &str) -> Result<Arc<Server<SharedFile>>, String> {
    match provide.server() {
        Some(server) if server.file.name == video => Ok(server),
        Some(_) => Err("File provider is currently providing another file. Try again later".into()),
        None => Err("Not providing any files".into()),
    }
}

/// The local video server plays a file another peer shares.
#[derive(Debug)]
pub struct FileShareConsumer {
    /// Where chunks are fetched from.
    provider: PeerId,
    chunk_requests: HashMap<OutboundRequestId, oneshot::Sender<Vec<u8>>>,
    /// A chunk request failed and none has succeeded since. The user hears
    /// of a stall once, however many chunks fail in it.
    stalled: bool,
}

impl Net {
    /// Tells the core that the stream its video server plays has ended.
    fn stream_stopped(&self) {
        self.message_sender.send(StreamEndedMsg.into()).ok();
    }
}

impl Session {
    pub(super) fn fs_req_resp_event(
        &mut self,
        event: request_response::Event<Tagged<FileShareRequest>, FileShareResponseResult>,
    ) {
        match event {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request: Tagged { tag, .. },
                    channel,
                    ..
                } if !self.net.membership.admits(&peer, &tag) => {
                    warn!(%peer, "Refusing a file share request of a peer outside the room");
                    let resp = Err(String::from("Not a member of the room"));
                    self.net.swarm.send_file_response(channel, resp).ok();
                }
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    self.fs_swarm_chunk_request(request.msg, channel).ok();
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => self.fs_swarm_response(request_id, response),
            },
            request_response::Event::OutboundFailure {
                request_id, error, ..
            } => {
                let Some(consumer) = &mut self.consumer else {
                    return;
                };
                if consumer.chunk_requests.remove(&request_id).is_none() {
                    return;
                }
                if !self.links.is_linked(&consumer.provider) {
                    // Typically a timeout because the hole punch to the
                    // provider never produced a direct connection.
                    warn!(
                        ?error,
                        "Chunk request failed without a link to the provider"
                    );
                    self.fs_stop_consuming("Could not reach the provider of the file");
                } else if !std::mem::replace(&mut consumer.stalled, true) {
                    warn!(?error, "Chunk request failed. Cache will retry.");
                    self.net
                        .tell_user("Chunk download timeout. Connection might be slow...");
                }
            }
            _ => {}
        }
    }

    /// Ends the stream this peer plays if `peer` provided it: with the link
    /// to the provider gone, no chunk request can be answered.
    pub(super) fn fs_peer_unlinked(&mut self, peer: PeerId) {
        let Some(consumer) = &self.consumer else {
            return;
        };
        if consumer.provider != peer {
            return;
        }
        warn!(%peer, "The provider of the file disconnected");
        self.fs_stop_consuming("The provider of the file disconnected");
    }

    /// Ends the stream this peer plays and tells the user `why`. The chunk
    /// requests still pending go unanswered.
    fn fs_stop_consuming(&mut self, why: &str) {
        self.consumer.take();
        self.net.tell_user(why);
        self.net.stream_stopped();
    }

    /// The core ended the stream this peer plays. The chunk requests still
    /// pending go unanswered.
    pub(super) fn fs_stream_ended(&mut self) -> Result<()> {
        self.consumer.take();
        Ok(())
    }

    /// Whether another chunk of the local video server may be requested from
    /// the provider, see [`CHUNK_FETCHES`].
    pub(super) fn can_fetch_chunk(&self) -> bool {
        match &self.consumer {
            Some(consumer) => consumer.chunk_requests.len() < CHUNK_FETCHES,
            None => true,
        }
    }

    pub(super) fn fs_fetch_chunk(&mut self, request: ChunkRequest) -> Result<()> {
        let Some(consumer) = &mut self.consumer else {
            self.net.stream_stopped();
            bail!("No active file share consumer");
        };
        let ChunkRequest {
            file_name,
            start,
            len,
            reply,
        } = request;
        let msg = FileShareRequest {
            video: file_name,
            start,
            len,
        };
        let request_id = self.net.send_file_request(&consumer.provider, msg);
        consumer.chunk_requests.insert(request_id, reply);
        Ok(())
    }

    pub(super) fn fs_chunk_read(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        bytes: Option<Vec<u8>>,
    ) -> Result<()> {
        let response = match bytes {
            Some(bytes) => Ok(ChunkBytes(bytes)),
            None => Err(String::from("Not providing the requested file")),
        };
        self.net.swarm.send_file_response(channel, response)
    }

    /// Fetches the chunks of the local video server from the provider the
    /// core picked, in place of whatever stream this peer played.
    pub(super) fn fs_stream(&mut self, msg: StreamMsg) -> Result<()> {
        let provider = msg.provider;
        debug!(%provider, "Streaming from provider");
        let net = &mut self.net;
        // A provider without a connection yet is dialed over the host's
        // circuit, the rendezvous for the hole punch.
        if provider != net.host && !net.swarm.is_connected(&provider) {
            match net.host_circuit() {
                Some(circuit) => {
                    if let Err(err) = net.swarm.dial(circuit.with(Protocol::P2p(provider))) {
                        error!(?err, "Failed to dial file provider via host relay");
                    }
                }
                None => error!("Host address unknown, cannot dial file provider via host relay"),
            }
        }
        self.consumer = Some(FileShareConsumer {
            provider,
            chunk_requests: HashMap::default(),
            stalled: false,
        });
        Ok(())
    }

    /// Hands the request to the local video provider. When its queue is
    /// full, the request is dropped unanswered; the requester sees a failed
    /// request and its video server asks again later.
    fn fs_swarm_chunk_request(
        &mut self,
        request: FileShareRequest,
        channel: ResponseChannel<FileShareResponseResult>,
    ) -> Result<()> {
        let FileShareRequest { video, start, len } = request;
        let server = match server_of(&self.net.provide, &video) {
            Ok(server) => server,
            Err(reason) => {
                debug!(%video, reason, "Declining chunk request");
                return self.net.swarm.send_file_response(channel, Err(reason));
            }
        };
        let (reply, bytes) = oneshot::channel();
        let request = ChunkRequest {
            file_name: video,
            start,
            len,
            reply,
        };
        match server.sender.try_send(request) {
            Ok(()) => {
                let read = async move { (channel, bytes.await.ok()) };
                self.net.chunk_reads.push(read.boxed());
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                debug!("Video provider is busy, dropping chunk request");
                Ok(())
            }
            Err(TrySendError::Closed(_)) => {
                debug!("The video provider stopped reading the file");
                let resp = Err(String::from("Not providing the requested file"));
                self.net.swarm.send_file_response(channel, resp)
            }
        }
    }

    fn fs_swarm_response(&mut self, request_id: OutboundRequestId, resp: FileShareResponseResult) {
        // the answer to a request of an earlier stream says nothing about
        // the current one
        let Some(consumer) = &mut self.consumer else {
            return;
        };
        let Some(reply) = consumer.chunk_requests.remove(&request_id) else {
            return debug!("Ignoring the answer to a request no longer pending");
        };
        match resp {
            Ok(ChunkBytes(bytes)) => {
                consumer.stalled = false;
                reply.send(bytes).ok();
            }
            Err(error) => {
                warn!(%error, "The provider turned a request down");
                self.fs_stop_consuming(&error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::chunk::CHUNK_SIZE;
    use tokio::sync::mpsc;

    use super::*;

    /// A chunk takes its own size on the wire and a few bytes of framing.
    #[test]
    fn a_chunk_is_encoded_as_its_bytes() {
        let bytes: Vec<u8> = (0..CHUNK_SIZE).map(|i| i as u8).collect();
        let response: FileShareResponseResult = Ok(ChunkBytes(bytes.clone()));

        let encoded = cbor4ii::serde::to_vec(Vec::new(), &response).unwrap();
        assert!(
            encoded.len() <= bytes.len() + 32,
            "{} bytes on the wire for a chunk of {}",
            encoded.len(),
            bytes.len()
        );

        let decoded: FileShareResponseResult = cbor4ii::serde::from_slice(&encoded).unwrap();
        let Ok(ChunkBytes(received)) = decoded else {
            panic!("the chunk came back as something else");
        };
        assert_eq!(received, bytes);
    }

    /// A request is served exactly when the provide route serves its file.
    #[test]
    fn a_request_is_served_for_the_file_the_route_serves() {
        let provide = ProvideRoute::default();
        assert!(server_of(&provide, "ep1").is_err(), "nothing is offered");

        let (sender, _requests) = mpsc::channel(1);
        let file = SharedFile {
            name: "ep1".into(),
            size: 1_000,
        };
        provide.open(file.clone(), sender);
        assert_eq!(server_of(&provide, "ep1").unwrap().file, file);
        assert!(
            server_of(&provide, "ep2").is_err(),
            "another file is offered"
        );

        provide.close();
        assert!(
            server_of(&provide, "ep1").is_err(),
            "the offer was withdrawn"
        );
    }
}
