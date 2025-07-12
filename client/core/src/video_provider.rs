use std::io::{ErrorKind, SeekFrom};

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use tracing::{trace, warn};
const CHUNK_SIZE: usize = 512_000;

use crate::{CoreModel, EventHandler, FileEntry, VideoShareMsg};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoProviderTrait: std::fmt::Debug + Send {
    fn start_providing(&mut self, file: FileEntry);
    fn stop_providing(&mut self);
    fn request_chunk(&mut self, uuid: uuid::Uuid, file_name: &str, start: u64, len: u64);
    fn size(&self) -> Option<u64>;
    fn sharing(&self) -> bool;
    fn file_name(&self) -> Option<ArcStr>;
    async fn event(&mut self) -> VideoProviderEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum VideoProviderEvent {
    ChunkResponse,
    FileReady,
}

#[derive(Clone)]
pub struct ChunkResponse {
    pub uuid: uuid::Uuid,
    pub file_name: ArcStr,
    pub start: u64,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for ChunkResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkResponse")
            .field("uuid", &self.uuid)
            .field("file_name", &self.file_name)
            .field("start", &self.start)
            .field("bytes", &"[not shown]")
            .finish()
    }
}

impl EventHandler for ChunkResponse {
    fn handle(self, model: &mut CoreModel) {
        trace!("video provider chunk response");
        model
            .communicator
            .send(crate::OutgoingMessage::ChunkResponse(
                crate::ChunkResponseMsg {
                    uuid: self.uuid,
                    actor: Some(model.config.username.clone()),
                    video: (&self.file_name).into(),
                    start: self.start,
                    bytes: self.bytes,
                },
            ));
    }
}

#[derive(Debug, Clone)]
pub struct FileReady {
    pub file_name: ArcStr,
    pub size: u64,
}

impl EventHandler for FileReady {
    fn handle(self, model: &mut CoreModel) {
        trace!("video provider ready");
        model.ui.video_share(true);
        model.communicator.send(
            VideoShareMsg {
                video: Some((&self.file_name).into()),
            }
            .into(),
        )
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

    fn request_chunk(&mut self, uuid: uuid::Uuid, file_name: &str, start: u64, len: u64) {
        let Some(handle) = self.file_handle.as_mut() else {
            return;
        };
        if !handle.file_name.eq(file_name) {
            return;
        }
        handle.send(Request { uuid, start, len })
    }

    fn size(&self) -> Option<u64> {
        self.file_handle.as_ref().and_then(|f| f.size)
    }

    fn sharing(&self) -> bool {
        self.file_handle.is_some()
    }

    fn file_name(&self) -> Option<ArcStr> {
        self.file_handle.as_ref().map(|f| f.file_name.clone())
    }

    async fn event(&mut self) -> VideoProviderEvent {
        let Some(handle) = self.file_handle.as_mut() else {
            std::future::pending().await
        };
        handle.event().await
    }
}

struct Request {
    uuid: uuid::Uuid,
    start: u64,
    len: u64,
}

struct Response {
    uuid: uuid::Uuid,
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
            size: None,
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
        let uuid = request.uuid;
        Response { uuid, start, bytes }
    }
}

#[derive(Debug)]
struct FileHandle {
    file_name: ArcStr,
    file_rx: Receiver<u64>,
    size: Option<u64>,
    req_tx: UnboundedSender<Request>,
    resp_rx: UnboundedReceiver<Response>,
}

impl FileHandle {
    async fn event(&mut self) -> VideoProviderEvent {
        tokio::select! {
            Some(size) = self.file_rx.recv() => {
                self.size = Some(size);
                FileReady {
                    file_name: self.file_name.clone(),
                    size,
                }.into()
            }
            // TODO what to do if we receive `None` here
            // TODO this can only happen if the FileServer died
            Some(Response { uuid, start, bytes }) = self.resp_rx.recv() => {
                ChunkResponse {
                    uuid,
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
