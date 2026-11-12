use std::net::SocketAddr;

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tracing::trace;

use crate::{CoreModel, EventHandler, FilePathSearch, MediaPlayerTrait};

/// Serves a video that another peer provides to the local player. The
/// server fetches its chunks over a [`crate::chunk::ChunkRoute`] given at
/// construction.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoServerTrait: std::fmt::Debug + Send {
    fn stop_server(&mut self);
    fn start_server(&mut self, file_name: ArcStr, file_size: u64);
    fn addr(&self) -> Option<SocketAddr>;
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

impl EventHandler for ServerOnline {
    fn handle(self, model: &mut CoreModel) {
        trace!("video server online");
        let file = VideoServerFile::from(self);
        model.player.reload_video(&file, &file.file_name);
    }
}

pub struct VideoServerFile {
    file_name: ArcStr,
    addr: SocketAddr,
}

impl FilePathSearch for VideoServerFile {
    fn get_file_path(&self, filename: &str) -> Option<String> {
        if self.file_name.eq(filename) {
            return Some(format!("http://{}", self.addr));
        }
        None
    }
}

impl From<ServerOnline> for VideoServerFile {
    fn from(server: ServerOnline) -> Self {
        Self {
            file_name: server.file_name,
            addr: server.addr,
        }
    }
}
