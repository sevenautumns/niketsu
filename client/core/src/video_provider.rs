use std::io::{ErrorKind, SeekFrom};

use arcstr::ArcStr;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{trace, warn};

use crate::chunk::{CHUNK_SIZE, ChunkRequest, ProvideRoute, SharedFile};
use crate::ui::PlayerMessage;
use crate::{CoreModel, EventHandler, FileEntry};

/// Chunk requests waiting for the shared file to be read. The communicator
/// turns away requests beyond that.
const QUEUED_REQUESTS: usize = 16;

/// Provides a local file to other peers. Once the file is open, it serves
/// the [`ProvideRoute`] given at construction with the file's name and
/// size; what this peer offers is what that route serves.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait VideoProviderTrait: std::fmt::Debug + Send {
    fn start_providing(&mut self, file: FileEntry);
    fn stop_providing(&mut self);
    /// Whether a file is being provided, also while it is still opening.
    fn sharing(&self) -> bool;
    /// The file being provided, also while it is still opening.
    fn file_name(&self) -> Option<ArcStr>;
    /// The file other peers can stream from this one: the file the route
    /// serves.
    fn offered(&self) -> Option<SharedFile>;
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

/// The room learns of the offer from this peer's status.
impl EventHandler for FileReady {
    fn handle(self, model: &mut CoreModel) {
        trace!("video provider ready");
        model.ui.video_share(true);
        model.publish_status();
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
        model.ui.player_message(PlayerMessage::error(
            "Stopped sharing: failed to read the shared file",
        ));
    }
}

#[derive(Debug)]
pub struct VideoProvider {
    route: ProvideRoute,
    file_handle: Option<FileHandle>,
}

impl VideoProvider {
    pub fn new(route: ProvideRoute) -> Self {
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
        self.file_handle = Some(FileServer::run(file));
    }

    fn stop_providing(&mut self) {
        self.file_handle.take();
        self.route.close();
    }

    fn sharing(&self) -> bool {
        self.file_handle.is_some()
    }

    fn file_name(&self) -> Option<ArcStr> {
        self.file_handle.as_ref().map(|f| f.file_name.clone())
    }

    fn offered(&self) -> Option<SharedFile> {
        self.route.server().map(|server| server.file.clone())
    }

    /// Opens the route once the file is open, so the offer carries its
    /// size.
    async fn event(&mut self) -> VideoProviderEvent {
        let Some(handle) = self.file_handle.as_mut() else {
            std::future::pending().await
        };
        let Some(size) = handle.file_rx.recv().await else {
            self.stop_providing();
            return SharingStopped.into();
        };
        let file_name = handle.file_name.clone();
        let file = SharedFile {
            name: file_name.clone(),
            size,
        };
        self.route.open(file, handle.requests.clone());
        FileReady { file_name, size }.into()
    }
}

struct FileServer;

impl FileServer {
    /// Opens the file and reads the chunks of the requests sent to the
    /// handle, one at a time, until no request can come anymore.
    fn run(file: FileEntry) -> FileHandle {
        let (file_tx, file_rx) = mpsc::channel(1);
        let (requests, mut queue) = mpsc::channel::<ChunkRequest>(QUEUED_REQUESTS);
        let file_name = file.file_name_arc();

        // On any I/O error the task returns, which closes the size channel
        // and surfaces as a `SharingStopped` event in `VideoProvider::event`.
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
            while let Some(request) = queue.recv().await {
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
            requests,
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
    /// The file's size once it is open. The channel closes when the file
    /// server task died (e.g. the file disappeared or became unreadable)
    /// and sharing has to stop.
    file_rx: Receiver<u64>,
    /// Where the chunk requests for the task go.
    requests: Sender<ChunkRequest>,
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

    /// The route offers the file with its size once the file is open, its
    /// requests are answered from the file, and stopping closes it.
    #[tokio::test]
    async fn test_the_route_offers_the_open_file_and_answers_from_it() {
        let path =
            std::env::temp_dir().join(format!("niketsu-route-test-{}", uuid::Uuid::new_v4()));
        let content: Vec<u8> = (0..1000u32).flat_map(u32::to_le_bytes).collect();
        tokio::fs::write(&path, &content).await.unwrap();

        let route = ProvideRoute::default();
        let mut provider = VideoProvider::new(route.clone());
        provider.start_providing(FileEntry::new("shared.mkv".into(), path.clone(), None));
        assert!(provider.sharing());
        assert!(route.server().is_none(), "offered before the file is open");
        assert_eq!(provider.offered(), None);

        let event = provider.event().await;
        assert!(matches!(event, VideoProviderEvent::FileReady(_)));
        let server = route.server().expect("the open file is not offered");
        let shared = SharedFile {
            name: "shared.mkv".into(),
            size: content.len() as u64,
        };
        assert_eq!(server.file, shared);
        assert_eq!(provider.offered(), Some(shared));

        let (request, bytes) = chunk_request("shared.mkv", 100, 50);
        server.sender.send(request).await.unwrap();
        assert_eq!(bytes.await.unwrap(), content[100..150]);

        drop(server);
        provider.stop_providing();
        assert!(route.server().is_none());
        assert_eq!(provider.offered(), None);
        tokio::fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn test_missing_file_stops_sharing() {
        let route = ProvideRoute::default();
        let mut provider = VideoProvider::new(route.clone());
        provider.start_providing(FileEntry::new(
            "missing.mkv".into(),
            "/nonexistent/missing.mkv".into(),
            None,
        ));
        assert!(provider.sharing());

        let event = provider.event().await;
        assert!(matches!(event, VideoProviderEvent::SharingStopped(_)));
        assert!(!provider.sharing());
        assert!(route.server().is_none());
    }
}
