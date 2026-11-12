//! The data path of peer-to-peer streaming. Chunk requests go from the video
//! server to the communicator on the viewer side, and from the communicator
//! to the video provider on the provider side, without passing the core
//! loop. Every queue on the way is bounded, so a slow consumer slows the
//! producer.

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use arcstr::ArcStr;
use tokio::sync::{mpsc, oneshot};

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
/// moment. Clones share the route, so it is created once at wiring time and
/// handed to both ends.
#[derive(Debug, Clone, Default)]
pub struct ChunkRoute(Arc<ArcSwapOption<mpsc::Sender<ChunkRequest>>>);

impl ChunkRoute {
    /// Serves the route from a fresh channel of `capacity` requests, which
    /// replaces the channel of the previous server.
    pub fn serve(&self, capacity: usize) -> mpsc::Receiver<ChunkRequest> {
        let (sender, receiver) = mpsc::channel(capacity);
        self.0.store(Some(Arc::new(sender)));
        receiver
    }

    /// Closes the route; the current server's channel ends once no request
    /// is being sent anymore.
    pub fn close(&self) {
        self.0.store(None);
    }

    pub fn sender(&self) -> Option<Arc<mpsc::Sender<ChunkRequest>>> {
        self.0.load_full()
    }
}
