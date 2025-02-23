use std::io::{ErrorKind, SeekFrom};

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use tracing::{trace, warn};
const CHUNK_SIZE: usize = 512_000;

use crate::{CoreModel, EventHandler, FileEntry};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoProviderTrait: std::fmt::Debug + Send {
    fn start_providing(&mut self, file: FileEntry);
    fn stop_providing(&mut self);
    fn request_chunk(&mut self, file_name: &str, start: u64, len: u64);
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
    pub size: u64,
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
    fn start_providing(&mut self, file: FileEntry) {
        self.stop_providing();
        let handle = FileServer::run(file);
        self.file_handle = Some(handle);
    }

    fn stop_providing(&mut self) {
        self.file_handle.take();
    }

    fn request_chunk(&mut self, file_name: &str, start: u64, len: u64) {
        let Some(handle) = self.file_handle.as_mut() else {
            return;
        };
        if !handle.file_name.eq(file_name) {
            return;
        }
        handle.send(Request { start, len })
    }

    async fn event(&mut self) -> VideoProviderEvent {
        let Some(handle) = self.file_handle.as_mut() else {
            std::future::pending().await
        };
        handle.event().await
    }
}

struct Request {
    start: u64,
    len: u64,
}

struct Response {
    start: u64,
    bytes: Vec<u8>,
}

struct FileServer;

impl FileServer {
    fn run(file: FileEntry) -> FileHandle {
        let (req_tx, mut req_rx) = tokio::sync::mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let (file_tx, file_rx) = tokio::sync::mpsc::channel(1);
        let file_name = file.file_name_arc();

        tokio::spawn(async move {
            let file = tokio::fs::File::open(file.path()).await.unwrap();
            let file_size = file.metadata().await.unwrap().len();
            file_tx.send(file_size).await.unwrap();
            let mut reader = BufReader::new(file);
            while let Some(req) = req_rx.recv().await {
                let resp = Self::handle_request(req, &mut reader).await;
                resp_tx.send(resp).ok();
            }
        });
        FileHandle {
            file_name,
            file_rx,
            req_tx,
            resp_rx,
        }
    }

    async fn handle_request(request: Request, reader: &mut BufReader<File>) -> Response {
        let len = CHUNK_SIZE.min(request.len as usize);
        let mut bytes = vec![0; len];
        reader.seek(SeekFrom::Start(request.start)).await.unwrap();
        let read = reader.read_exact(&mut bytes).await;
        match read {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                reader.seek(SeekFrom::Start(request.start)).await.unwrap();
                let len = reader.read_to_end(&mut bytes).await.unwrap();
                bytes.truncate(len);
            }
            err @ Err(_) => err.map(|_| ()).unwrap(),
            Ok(_) => {}
        }
        let start = request.start;
        Response { start, bytes }
    }
}

#[derive(Debug)]
struct FileHandle {
    file_name: ArcStr,
    file_rx: Receiver<u64>,
    req_tx: UnboundedSender<Request>,
    resp_rx: UnboundedReceiver<Response>,
}

impl FileHandle {
    async fn event(&mut self) -> VideoProviderEvent {
        tokio::select! {
            Some(size) = self.file_rx.recv() => {
                FileReady {
                    file_name: self.file_name.clone(),
                    size,
                }.into()
            }
            // TODO what to do if we receive `None` here
            // TODO this can only happen if the FileServer died
            Some(Response { start, bytes }) = self.resp_rx.recv() => {
                ChunkResponse {
                    file_name: self.file_name.clone(),
                    start,
                    bytes,
                }.into()
            }
        }
    }

    fn send(&mut self, request: Request) {
        if let Err(err) = self.req_tx.send(request) {
            warn!(?err, "failed to send request")
        }
    }
}
