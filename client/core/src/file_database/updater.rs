use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;

use anyhow::Result;
use futures::stream::FusedStream;
use futures::{Stream, StreamExt};
use log::warn;
use tokio::fs::{DirEntry, ReadDir};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::UpdateProgressTracker;
use crate::file_database::FileEntry;

pub const MAX_CONCURRENT_CRAWLER: usize = 100;

pub(super) struct FileDatabaseUpdater {
    path: PathBuf,
    semaphore: Arc<Semaphore>,
    paths: Vec<FileEntry>,
    progress: Arc<UpdateProgressTracker>,
    subdirs: JoinSet<Result<Vec<FileEntry>>>,
}

impl FileDatabaseUpdater {
    // TODO extract into its own struct
    pub(super) async fn update_all(
        paths: impl Iterator<Item = PathBuf>,
        progress: Arc<UpdateProgressTracker>,
    ) -> Vec<FileEntry> {
        let mut updater = JoinSet::default();
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CRAWLER));
        for path in paths {
            updater.spawn(
                Self::new(path.to_path_buf(), progress.clone(), semaphore.clone()).complete(),
            );
        }
        let mut database = Vec::new();
        while let Some(res) = updater.join_next().await {
            match res {
                Ok(Err(err)) => warn!("{err:?}"),
                Err(err) => warn!("{err:?}"),
                Ok(Ok(db)) => database.extend_from_slice(&db),
            }
        }
        database
    }

    fn new(path: PathBuf, progress: Arc<UpdateProgressTracker>, semaphore: Arc<Semaphore>) -> Self {
        Self {
            path,
            progress,
            semaphore,
            subdirs: JoinSet::default(),
            paths: Vec::default(),
        }
    }

    fn clone_with(&self, path: PathBuf) -> Self {
        Self {
            path,
            semaphore: self.semaphore.clone(),
            progress: self.progress.clone(),
            subdirs: JoinSet::default(),
            paths: Vec::default(),
        }
    }

    async fn complete(mut self) -> Result<Vec<FileEntry>> {
        self.progress.inc_queued();

        self.crawl_dir().await?;
        self.finish_subdirs().await;

        self.progress.inc_finished();
        Ok(self.paths)
    }

    async fn crawl_dir(&mut self) -> Result<()> {
        let permit = self.semaphore.clone().acquire_owned();
        let mut dir = FusedReadDir::new(self.path.clone()).await?;
        while let Some(entry) = dir.next().await {
            self.handle_entry(entry).await;
        }
        drop(permit);
        Ok(())
    }

    async fn handle_entry(&mut self, entry: DirEntry) {
        let Ok(typ) = entry.file_type().await else {
            return;
        };
        if typ.is_dir() {
            self.spawn_subdir_crawler(entry.path())
        } else if typ.is_file() {
            self.insert_file(entry).await
        }
    }

    fn spawn_subdir_crawler(&mut self, path: PathBuf) {
        let subdir = self.clone_with(path).complete();
        self.subdirs.spawn(subdir);
    }

    async fn insert_file(&mut self, file: DirEntry) {
        let name = file.file_name().to_string_lossy().into();
        let path = file.path();
        let modified = file
            .metadata()
            .await
            .ok()
            .and_then(|meta| meta.modified().ok());
        self.paths.push(FileEntry::new(name, path, modified));
    }

    async fn finish_subdirs(&mut self) {
        while let Some(subdir) = self.subdirs.join_next().await {
            match subdir {
                Ok(Err(err)) => warn!("{err:?}"),
                Err(err) => warn!("{err:?}"),
                Ok(Ok(paths)) => self.paths.extend(paths),
            }
        }
    }
}

#[derive(Debug)]
struct FusedReadDir {
    dir: ReadDir,
    ended: bool,
}

impl FusedReadDir {
    pub async fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            dir: tokio::fs::read_dir(path).await?,
            ended: false,
        })
    }
}

impl Stream for FusedReadDir {
    type Item = DirEntry;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let reader = std::pin::Pin::<&mut FusedReadDir>::into_inner(self);
        let entry = reader.dir.poll_next_entry(cx);
        let Poll::Ready(entry) = entry else {
            return Poll::Pending;
        };
        let entry = entry.unwrap_or_else(|e| {
            warn!("{e:?}");
            None
        });
        if entry.is_none() {
            reader.ended = true;
        }
        Poll::Ready(entry)
    }
}

impl FusedStream for FusedReadDir {
    fn is_terminated(&self) -> bool {
        self.ended
    }
}
