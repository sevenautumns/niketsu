use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use arcstr::ArcStr;
use futures::FutureExt;
use futures::future::BoxFuture;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use libp2p::swarm::DialError;
use libp2p::swarm::dial_opts::DialOpts;
use niketsu_core::chunk::{ChunkFetch, ChunkRequest, FetchRoute, ProvideRoute, Server, SharedFile};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use super::member::Tagged;
use super::{Session, SwarmExt};

/// The chunk routes of this peer, see [`niketsu_core::chunk`].
#[derive(Debug, Clone)]
pub(crate) struct ChunkRoutes {
    /// The chunks the local player reads of other peers' files, which the
    /// communicator fetches from them.
    pub fetch: FetchRoute,
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

/// The chunks asked of other peers and not yet answered, with where the
/// bytes go. A request that fails or is turned down drops its entry, which
/// tells the reader that waits for it.
pub(super) type PendingChunks = HashMap<OutboundRequestId, oneshot::Sender<Vec<u8>>>;

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
                peer,
                request_id,
                error,
                ..
            } => {
                warn!(%peer, ?error, "A chunk request failed");
                self.pending_chunks.remove(&request_id);
            }
            _ => {}
        }
    }

    /// Asks the provider of a file for a chunk of it. A provider this peer
    /// has no connection to is dialed over the host's circuit, the
    /// rendezvous for the hole punch; the request waits for the direct
    /// connection and fails with the dial.
    pub(super) fn fs_fetch_chunk(&mut self, fetch: ChunkFetch) {
        let ChunkFetch { provider, request } = fetch;
        let ChunkRequest {
            file_name,
            start,
            len,
            reply,
        } = request;
        let net = &mut self.net;
        if !net.swarm.is_connected(&provider)
            && let Some(circuit) = net.host_circuit()
        {
            debug!(%provider, "Dialing the provider of a file over the host's circuit");
            let dial = DialOpts::peer_id(provider)
                .addresses(vec![circuit.with(Protocol::P2p(provider))])
                .build();
            match net.swarm.dial(dial) {
                // the dial of an earlier request is still on its way
                Ok(()) | Err(DialError::DialPeerConditionFalse(_)) => {}
                Err(error) => warn!(%provider, %error, "Failed to dial the provider of a file"),
            }
        }
        let msg = FileShareRequest {
            video: file_name,
            start,
            len,
        };
        let request_id = net.send_file_request(&provider, msg);
        self.pending_chunks.insert(request_id, reply);
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

    /// Hands the request to the local video provider. When its queue is
    /// full, the request is turned down, and the requester's reader asks
    /// again.
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
                debug!("Video provider is busy, declining chunk request");
                let resp = Err(String::from("The provider is busy"));
                self.net.swarm.send_file_response(channel, resp)
            }
            Err(TrySendError::Closed(_)) => {
                debug!("The video provider stopped reading the file");
                let resp = Err(String::from("Not providing the requested file"));
                self.net.swarm.send_file_response(channel, resp)
            }
        }
    }

    fn fs_swarm_response(&mut self, request_id: OutboundRequestId, resp: FileShareResponseResult) {
        let Some(reply) = self.pending_chunks.remove(&request_id) else {
            return debug!("Ignoring the answer to a request no longer pending");
        };
        match resp {
            // a reader that gave up waiting is gone
            Ok(ChunkBytes(bytes)) => drop(reply.send(bytes)),
            Err(error) => warn!(%error, "The provider turned a chunk request down"),
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
