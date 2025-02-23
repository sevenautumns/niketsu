use std::path::Path;
use std::{io::ErrorKind, ops::RangeInclusive};

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use tracing::{trace, warn};
const CHUNK_SIZE: u64 = 512_000;

use crate::{CoreModel, EventHandler};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoProviderTrait: std::fmt::Debug + Send {
    fn start_providing(&mut self, file_name: ArcStr);
    fn stop_providing(&mut self);
    fn request_chunk(&mut self, file_name: &str, request: RangeInclusive<u64>);
    async fn event(&mut self) -> VideoProviderEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum VideoProviderEvent {
    ChunkResponse,
    FileReady,
}

#[derive(Debug, Clone)]
pub struct ChunkResponse {
    pub file_name: ArcStr,
    pub start: u64,
    pub bytes: Vec<u8>,
}

impl EventHandler for ChunkResponse {
    fn handle(self, _model: &mut CoreModel) {
        trace!("video provider chunk response");
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct FileReady {
    pub file_name: ArcStr,
}

impl EventHandler for FileReady {
    fn handle(self, _model: &mut CoreModel) {
        trace!("video provider ready");
        todo!()
    }
}

#[derive(Debug, Default)]
pub struct VideoProvider {
    file_handle: Option<FileHandle>,
}

#[async_trait]
impl VideoProviderTrait for VideoProvider {
    fn start_providing(&mut self, file_name: ArcStr) {
        self.stop_providing();
        let (req_tx, req_rx) = tokio::sync::mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let file_server = FileServer::new(file_name, req_rx, resp_tx);
        let handle = file_server.run(req_tx, resp_rx);
        self.file_handle = Some(handle);
    }

    fn stop_providing(&mut self) {
        self.file_handle.take();
    }

    fn request_chunk(&mut self, file_name: &str, request: RangeInclusive<u64>) {
        let Some(handle) = self.file_handle.as_mut() else {
            return;
        };
        if !handle.file_name.eq(file_name) {
            return;
        }
        handle.send(request)
    }

    async fn event(&mut self) -> VideoProviderEvent {
        let Some(handle) = self.file_handle.as_mut() else {
            std::future::pending().await
        };
        handle.event().await
    }
}

struct FileServer {
    file_name: ArcStr,
    reader: Option<tokio::io::BufReader<tokio::fs::File>>,
    buf: Vec<u8>,
    req_rx: UnboundedReceiver<RangeInclusive<u64>>,
    resp_tx: UnboundedSender<(RangeInclusive<u64>, Vec<u8>)>,
}

impl FileServer {
    fn new(
        file_name: ArcStr,
        req_rx: UnboundedReceiver<RangeInclusive<u64>>,
        resp_tx: UnboundedSender<(RangeInclusive<u64>, Vec<u8>)>,
    ) -> FileServer {
        let buf = vec![0; CHUNK_SIZE as usize];
        Self {
            file_name,
            reader: None,
            buf,
            req_rx,
            resp_tx,
        }
    }

    fn run(
        mut self,
        req_tx: UnboundedSender<RangeInclusive<u64>>,
        resp_rx: UnboundedReceiver<(RangeInclusive<u64>, Vec<u8>)>,
    ) -> FileHandle {
        let (file_tx, file_rx) = tokio::sync::mpsc::channel(1);
        let file_name = self.file_name.clone();

        tokio::spawn(async move {
            self.open_file().await;
            file_tx.send(true).await.unwrap();
            loop {
                tokio::select! {
                    Some(req) = self.req_rx.recv() => {
                        self.handle_request(req).await;
                        todo!();
                        // should be run async, so it does not block?
                    }
                }
            }
        });
        FileHandle {
            file_name,
            file_rx,
            req_tx,
            resp_rx,
        }
    }

    async fn open_file(&mut self) {
        let file = tokio::fs::File::open(Path::new(self.file_name.as_str()))
            .await
            .unwrap();
        let reader = BufReader::new(file);
        self.reader = Some(reader);
    }

    async fn handle_request(&mut self, request: RangeInclusive<u64>) {
        let Some(reader) = &mut self.reader else {
            return;
        };

        reader
            .seek(std::io::SeekFrom::Start(*request.start() as u64))
            .await
            .unwrap();
        let read = reader.read_exact(&mut self.buf).await;
        if let Err(err) = read {
            if err.kind() == ErrorKind::UnexpectedEof {
                reader
                    .seek(std::io::SeekFrom::Start(*request.start() as u64))
                    .await
                    .unwrap();
                reader.read_to_end(&mut self.buf).await.unwrap();
            }

            self.resp_tx
                .send((request.clone(), self.buf.clone()))
                .unwrap();
            self.buf.resize(CHUNK_SIZE as usize, 0);
        }
    }
}

#[derive(Debug)]
struct FileHandle {
    file_name: ArcStr,
    file_rx: Receiver<bool>,
    req_tx: UnboundedSender<RangeInclusive<u64>>,
    resp_rx: UnboundedReceiver<(RangeInclusive<u64>, Vec<u8>)>,
}

impl FileHandle {
    async fn event(&mut self) -> VideoProviderEvent {
        tokio::select! {
            Some(_) = self.file_rx.recv() => {
                FileReady {
                    file_name: self.file_name.clone(),
                }.into()
            }
            Some(resp) = self.resp_rx.recv() => {
                ChunkResponse {
                    file_name: self.file_name.clone(),
                    start: *resp.0.start() as u64,
                    bytes: resp.1
                }.into()
            }
        }
    }

    fn send(&mut self, request: RangeInclusive<u64>) {
        if let Err(err) = self.req_tx.send(request) {
            warn!(?err, "failed to send request")
        }
    }
}
