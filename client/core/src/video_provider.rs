use std::io::{ErrorKind, SeekFrom};

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender};
use tracing::{trace, warn};

use crate::ui::{MessageLevel, MessageSource, PlayerMessageInner};
use crate::{CoreModel, EventHandler, FileEntry, VideoShareMsg};

const CHUNK_SIZE: usize = 512_000;

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
    SharingStopped,
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

/// Emitted when the file server task stopped on its own, e.g. because the
/// shared file disappeared or became unreadable.
#[derive(Debug, Clone)]
pub struct SharingStopped;

impl EventHandler for SharingStopped {
    fn handle(self, model: &mut CoreModel) {
        trace!("video provider stopped sharing");
        model.stop_sharing();
        model.ui.player_message(
            PlayerMessageInner {
                message: "Stopped sharing: failed to read the shared file".into(),
                source: MessageSource::Internal,
                level: MessageLevel::Error,
                timestamp: Local::now(),
            }
            .into(),
        );
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
        match handle.event().await {
            Some(event) => event,
            None => {
                self.file_handle = None;
                SharingStopped.into()
            }
        }
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

        // On any I/O error the task returns, which closes both channels and
        // surfaces as a `SharingStopped` event in `VideoProvider::event`.
        tokio::spawn(async move {
            let path = file.path().to_path_buf();
            let file = match File::open(&path).await {
                Ok(file) => file,
                Err(error) => return warn!(%error, ?path, "failed to open shared file"),
            };
            let file_size = match file.metadata().await {
                Ok(metadata) => metadata.len(),
                Err(error) => return warn!(%error, ?path, "failed to read shared file metadata"),
            };
            if file_tx.send(file_size).await.is_err() {
                return;
            }
            let mut reader = BufReader::new(file);
            while let Some(req) = req_rx.recv().await {
                match Self::handle_request(req, &mut reader).await {
                    Ok(resp) => {
                        resp_tx.send(resp).ok();
                    }
                    Err(error) => return warn!(%error, ?path, "failed to read shared file"),
                }
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

    async fn handle_request(
        request: Request,
        reader: &mut BufReader<File>,
    ) -> std::io::Result<Response> {
        let len = CHUNK_SIZE.min(request.len as usize);
        let mut bytes = vec![0; len];
        reader.seek(SeekFrom::Start(request.start)).await?;
        let read = reader.read_exact(&mut bytes).await;
        match read {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // request reaches past EOF: return what the file still has
                bytes.clear();
                reader.seek(SeekFrom::Start(request.start)).await?;
                reader.read_to_end(&mut bytes).await?;
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        let start = request.start;
        let uuid = request.uuid;
        Ok(Response { uuid, start, bytes })
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
    /// `None` means the file server task died (e.g. the file disappeared or
    /// became unreadable) and sharing has to stop.
    async fn event(&mut self) -> Option<VideoProviderEvent> {
        tokio::select! {
            size = self.file_rx.recv() => {
                let size = size?;
                self.size = Some(size);
                Some(FileReady {
                    file_name: self.file_name.clone(),
                    size,
                }.into())
            }
            resp = self.resp_rx.recv() => {
                let Response { uuid, start, bytes } = resp?;
                Some(ChunkResponse {
                    uuid,
                    file_name: self.file_name.clone(),
                    start,
                    bytes,
                }.into())
            }
        }
    }

    fn send(&mut self, request: Request) {
        if let Err(err) = self.req_tx.send(request) {
            warn!(?err, "failed to send request")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chunk_request_past_eof_returns_file_tail() {
        let path = std::env::temp_dir().join(format!("niketsu-eof-test-{}", uuid::Uuid::new_v4()));
        let content: Vec<u8> = (0..1000u32).flat_map(u32::to_le_bytes).collect();
        tokio::fs::write(&path, &content).await.unwrap();

        let file = File::open(&path).await.unwrap();
        let mut reader = BufReader::new(file);
        let request = Request {
            uuid: uuid::Uuid::new_v4(),
            start: 3000,
            len: 2000,
        };
        let response = FileServer::handle_request(request, &mut reader)
            .await
            .unwrap();
        tokio::fs::remove_file(&path).await.ok();

        assert_eq!(response.start, 3000);
        assert_eq!(response.bytes, content[3000..]);
    }

    #[tokio::test]
    async fn test_missing_file_stops_sharing() {
        let mut provider = VideoProvider::default();
        provider.start_providing(FileEntry::new(
            "missing.mkv".into(),
            "/nonexistent/missing.mkv".into(),
            None,
        ));
        assert!(provider.sharing());

        let event = provider.event().await;
        assert!(matches!(event, VideoProviderEvent::SharingStopped(_)));
        assert!(!provider.sharing());
    }
}
