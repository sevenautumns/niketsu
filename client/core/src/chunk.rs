//! The data path of peer-to-peer streaming. Chunk requests go from the video
//! server to the communicator on the viewer side, and from the communicator
//! to the video provider on the provider side, without passing the core
//! loop. Every queue on the way is bounded, so a slow consumer slows the
//! producer.

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use arcstr::ArcStr;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

/// The most bytes one chunk carries. The video server asks for no more and
/// the video provider reads no more, so a chunk fits a response on the wire
/// whichever side is asked.
pub const CHUNK_SIZE: u64 = 512_000;

/// A byte range of a shared video. The bytes are sent on `reply`; dropping
/// `reply` means the range cannot be served.
#[derive(Debug)]
pub struct ChunkRequest {
    pub file_name: ArcStr,
    pub start: u64,
    pub len: u64,
    pub reply: oneshot::Sender<Vec<u8>>,
}

/// Where chunk requests go: the channel of whoever serves them at the
/// moment, and what it serves. Clones share the route, so it is created
/// once at wiring time and handed to both ends.
#[derive(Debug)]
pub struct ChunkRoute<F = ()>(Arc<ArcSwapOption<Server<F>>>);

/// Who serves a route: the requests go to `sender`, and it serves `file`.
#[derive(Debug)]
pub struct Server<F> {
    pub file: F,
    pub sender: mpsc::Sender<ChunkRequest>,
}

/// A file a peer offers to the others: what a viewer needs to stream it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SharedFile {
    pub name: ArcStr,
    pub size: u64,
}

/// The chunk requests of other peers, which go to the local video provider.
/// The provider opens the route once it can serve its file, so the route
/// holds whether and what this peer offers.
pub type ProvideRoute = ChunkRoute<SharedFile>;

impl<F> Default for ChunkRoute<F> {
    fn default() -> Self {
        Self(Arc::default())
    }
}

impl<F> Clone for ChunkRoute<F> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<F> ChunkRoute<F> {
    /// Serves the route through `sender` with `file`, in place of the
    /// previous server.
    pub fn open(&self, file: F, sender: mpsc::Sender<ChunkRequest>) {
        self.0.store(Some(Arc::new(Server { file, sender })));
    }

    /// Closes the route; the current server's channel ends once no request
    /// is being sent anymore.
    pub fn close(&self) {
        self.0.store(None);
    }

    pub fn server(&self) -> Option<Arc<Server<F>>> {
        self.0.load_full()
    }
}

impl ChunkRoute {
    /// Serves the route from a fresh channel of `capacity` requests, which
    /// replaces the channel of the previous server.
    pub fn serve(&self, capacity: usize) -> mpsc::Receiver<ChunkRequest> {
        let (sender, receiver) = mpsc::channel(capacity);
        self.open((), sender);
        receiver
    }

    pub fn sender(&self) -> Option<mpsc::Sender<ChunkRequest>> {
        self.server().map(|server| server.sender.clone())
    }
}
