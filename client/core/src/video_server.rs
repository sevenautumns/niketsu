use std::net::SocketAddr;

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::time::Instant;
use tracing::trace;

use crate::player::Source;
use crate::{CoreModel, EventHandler};

/// Serves a video that another peer provides to the local player. The
/// server fetches its chunks over a [`crate::chunk::ChunkRoute`] given at
/// construction.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoServerTrait: std::fmt::Debug + Send {
    fn stop_server(&mut self);
    fn start_server(&mut self, file_name: ArcStr, file_size: u64);
    async fn event(&mut self) -> VideoServerEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum VideoServerEvent {
    ServerOnline,
}

#[derive(Debug, Clone)]
pub struct ServerOnline {
    pub file_name: ArcStr,
    pub addr: SocketAddr,
}

/// The video server serves the selected video, which the player plays
/// from there.
impl EventHandler for ServerOnline {
    fn handle(self, model: &mut CoreModel) {
        trace!("video server online");
        let selected = model.state.video.as_ref();
        if selected.is_none_or(|video| video.as_str() != self.file_name) {
            return;
        }
        let source = Source::Url(format!("http://{}", self.addr));
        model.play(Some(source), Instant::now());
    }
}
