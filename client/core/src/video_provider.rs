use std::io::{ErrorKind, SeekFrom};

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{self, Receiver};
use tracing::{trace, warn};

use crate::chunk::{CHUNK_SIZE, ChunkRequest, ChunkRoute};
use crate::ui::{MessageLevel, MessageSource, PlayerMessageInner};
use crate::{CoreModel, EventHandler, FileEntry, VideoShareMsg};

/// Chunk requests waiting for the shared file to be read. The communicator
/// turns away requests beyond that.
const QUEUED_REQUESTS: usize = 16;

/// Provides a local file to other peers. While providing, it answers the
/// chunk requests of a [`ChunkRoute`] given at construction.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoProviderTrait: std::fmt::Debug + Send {
    fn start_providing(&mut self, file: FileEntry);
    fn stop_providing(&mut self);
    fn size(&self) -> Option<u64>;
    fn sharing(&self) -> bool;
    fn file_name(&self) -> Option<ArcStr>;
    async fn event(&mut self) -> VideoProviderEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum VideoProviderEvent {
    FileReady,
    SharingStopped,
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

#[derive(Debug)]
pub struct VideoProvider {
    route: ChunkRoute,
    file_handle: Option<FileHandle>,
}

impl VideoProvider {
    pub fn new(route: ChunkRoute) -> Self {
        Self {
            route,
            file_handle: None,
        }
    }
}

#[async_trait]
impl VideoProviderTrait for VideoProvider {
    fn start_providing(&mut self, file: FileEntry) {
        self.stop_providing();
        let requests = self.route.serve(QUEUED_REQUESTS);
        self.file_handle = Some(FileServer::run(file, requests));
    }

    fn stop_providing(&mut self) {
        self.file_handle.take();
        self.route.close();
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

struct FileServer;

impl FileServer {
    /// Reads the chunks of `requests` from the file, one at a time, until the
    /// route stops sending requests.
    fn run(file: FileEntry, mut requests: Receiver<ChunkRequest>) -> FileHandle {
        let (file_tx, file_rx) = mpsc::channel(1);
        let file_name = file.file_name_arc();

        // On any I/O error the task returns, which closes the size channel
        // and surfaces as a `SharingStopped` event in `VideoProvider::event`.
        tokio::spawn(async move {
            let name = file.file_name_arc();
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
            while let Some(request) = requests.recv().await {
                // a request for another file is dropped unanswered
                if request.file_name != name {
                    continue;
                }
                match Self::read_chunk(request.start, request.len, &mut reader).await {
                    Ok(bytes) => {
                        request.reply.send(bytes).ok();
                    }
                    Err(error) => return warn!(%error, ?path, "failed to read shared file"),
                }
            }
        });
        FileHandle {
            file_name,
            file_rx,
            size: None,
        }
    }

    async fn read_chunk(
        start: u64,
        len: u64,
        reader: &mut BufReader<File>,
    ) -> std::io::Result<Vec<u8>> {
        let len = CHUNK_SIZE.min(len) as usize;
        let mut bytes = vec![0; len];
        reader.seek(SeekFrom::Start(start)).await?;
        let read = reader.read_exact(&mut bytes).await;
        match read {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // request reaches past EOF: return what the file still has
                bytes.clear();
                reader.seek(SeekFrom::Start(start)).await?;
                reader.read_to_end(&mut bytes).await?;
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        Ok(bytes)
    }
}

#[derive(Debug)]
struct FileHandle {
    file_name: ArcStr,
    file_rx: Receiver<u64>,
    size: Option<u64>,
}

impl FileHandle {
    /// `None` means the file server task died (e.g. the file disappeared or
    /// became unreadable) and sharing has to stop.
    async fn event(&mut self) -> Option<VideoProviderEvent> {
        let size = self.file_rx.recv().await?;
        self.size = Some(size);
        Some(
            FileReady {
                file_name: self.file_name.clone(),
                size,
            }
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::*;

    #[tokio::test]
    async fn test_chunk_request_past_eof_returns_file_tail() {
        let path = std::env::temp_dir().join(format!("niketsu-eof-test-{}", uuid::Uuid::new_v4()));
        let content: Vec<u8> = (0..1000u32).flat_map(u32::to_le_bytes).collect();
        tokio::fs::write(&path, &content).await.unwrap();

        let file = File::open(&path).await.unwrap();
        let mut reader = BufReader::new(file);
        let bytes = FileServer::read_chunk(3000, 2000, &mut reader)
            .await
            .unwrap();
        tokio::fs::remove_file(&path).await.ok();

        assert_eq!(bytes, content[3000..]);
    }

    fn chunk_request(
        file_name: &str,
        start: u64,
        len: u64,
    ) -> (ChunkRequest, oneshot::Receiver<Vec<u8>>) {
        let (reply, bytes) = oneshot::channel();
        let file_name = file_name.into();
        (
            ChunkRequest {
                file_name,
                start,
                len,
                reply,
            },
            bytes,
        )
    }

    #[tokio::test]
    async fn test_route_requests_are_answered_from_the_shared_file() {
        let path =
            std::env::temp_dir().join(format!("niketsu-route-test-{}", uuid::Uuid::new_v4()));
        let content: Vec<u8> = (0..1000u32).flat_map(u32::to_le_bytes).collect();
        tokio::fs::write(&path, &content).await.unwrap();

        let route = ChunkRoute::default();
        let mut provider = VideoProvider::new(route.clone());
        provider.start_providing(FileEntry::new("shared.mkv".into(), path.clone(), None));
        let event = provider.event().await;
        assert!(matches!(event, VideoProviderEvent::FileReady(_)));

        let (request, bytes) = chunk_request("shared.mkv", 100, 50);
        route.sender().unwrap().send(request).await.unwrap();
        assert_eq!(bytes.await.unwrap(), content[100..150]);

        let (request, bytes) = chunk_request("other.mkv", 0, 10);
        route.sender().unwrap().send(request).await.unwrap();
        assert!(
            bytes.await.is_err(),
            "a request for another file is dropped"
        );

        provider.stop_providing();
        assert!(route.sender().is_none());
        tokio::fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn test_missing_file_stops_sharing() {
        let mut provider = VideoProvider::new(ChunkRoute::default());
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
