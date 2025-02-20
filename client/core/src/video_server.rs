use std::net::SocketAddr;

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tracing::trace;

use crate::{CoreModel, EventHandler};

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
    pub file_name: ArcStr,
    pub start: u64,
    pub length: u64,
}

impl EventHandler for ChunkRequest {
    fn handle(self, _model: &mut CoreModel) {
        trace!("video server chunk request");
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct ServerOnline {
    pub file_name: ArcStr,
    pub addr: SocketAddr,
}

impl EventHandler for ServerOnline {
    fn handle(self, _model: &mut CoreModel) {
        trace!("video server online");
        todo!()
    }
}
