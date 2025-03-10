use std::net::SocketAddr;
use std::ops::RangeInclusive;

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tracing::trace;

use crate::{
    ChunkRequestMsg, CoreModel, EventHandler, FilePathSearch, MediaPlayerTrait, OutgoingMessage,
};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoServerTrait: std::fmt::Debug + Send {
    fn stop_server(&mut self);
    fn start_server(&mut self, file_name: ArcStr, file_size: u64);
    fn insert_chunk(&mut self, file_name: &str, start: u64, bytes: Vec<u8>);
    fn addr(&self) -> Option<SocketAddr>;
    async fn event(&mut self) -> VideoServerEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum VideoServerEvent {
    ChunkRequest,
    ServerOnline,
}

#[derive(Debug, Clone)]
pub struct ChunkRequest {
    // TODO either use file_name or filename everywhere
    pub file_name: ArcStr,
    pub start: u64,
    pub length: u64,
}

impl EventHandler for ChunkRequest {
    fn handle(self, model: &mut CoreModel) {
        trace!("video server chunk request");
        model
            .communicator
            .send(OutgoingMessage::ChunkRequest(ChunkRequestMsg {
                uuid: uuid::Uuid::new_v4(),
                actor: Some(model.config.username.clone()),
                video: self.file_name.as_str().into(),
                range: RangeInclusive::new(self.start, self.start + self.length),
            }))
    }
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
        model.player.maybe_reload_video(&file);
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
