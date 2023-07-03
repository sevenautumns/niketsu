use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;

use anyhow::Result;
use dashmap::DashMap;
use futures::stream::FusedStream;
use futures::{Stream, StreamExt};
use log::warn;
use tokio::fs::{DirEntry, ReadDir};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::UpdateProgress;

pub const MAX_CONCURRENT_CRAWLER: usize = 100;

pub(super) struct FileDatabaseUpdater {
    path: PathBuf,
    semaphore: Arc<Semaphore>,
    data: Arc<DashMap<String, PathBuf>>,
    progress: Arc<UpdateProgress>,
    subdirs: JoinSet<Result<()>>,
}

impl FileDatabaseUpdater {
    // TODO extract into its own struct
    pub(super) async fn update_all(
        paths: impl Iterator<Item = PathBuf>,
        data: Arc<DashMap<String, PathBuf>>,
        progress: Arc<UpdateProgress>,
    ) {
        let mut updater = JoinSet::default();
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CRAWLER));
        for path in paths {
            updater.spawn(
                Self::new(
                    path.to_path_buf(),
                    data.clone(),
                    progress.clone(),
                    semaphore.clone(),
                )
                .complete(),
            );
        }
        while let Some(res) = updater.join_next().await {
            match res {
                Ok(Err(err)) => warn!("{err:?}"),
                Err(err) => warn!("{err:?}"),
                _ => {}
            }
        }
    }

    fn new(
        path: PathBuf,
        data: Arc<DashMap<String, PathBuf>>,
        progress: Arc<UpdateProgress>,
        semaphore: Arc<Semaphore>,
    ) -> Self {
        Self {
            path,
            data,
            progress,
            semaphore,
            subdirs: JoinSet::default(),
        }
    }

    fn clone_with(&self, path: PathBuf) -> Self {
        Self {
            path,
            semaphore: self.semaphore.clone(),
            data: self.data.clone(),
            progress: self.progress.clone(),
            subdirs: JoinSet::default(),
        }
    }

    async fn complete(mut self) -> Result<()> {
        self.progress.inc_queued();

        self.crawl_dir().await?;
        self.finish_subdirs().await;

        self.progress.inc_finished();
        Ok(())
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
            self.insert_file(entry)
        }
    }

    fn spawn_subdir_crawler(&mut self, path: PathBuf) {
        let subdir = self.clone_with(path).complete();
        self.subdirs.spawn(subdir);
    }

    fn insert_file(&self, file: DirEntry) {
        let name = file.file_name().to_string_lossy().to_string();
        let path = file.path();
        self.data.insert(name, path);
    }

    async fn finish_subdirs(&mut self) {
        while let Some(subdir) = self.subdirs.join_next().await {
            match subdir {
                Ok(Err(err)) => warn!("{err:?}"),
                Err(err) => warn!("{err:?}"),
                _ => {}
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
