use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use actix::{Actor, Context, SpawnHandle};
use anyhow::Result;
use async_recursion::async_recursion;
use dashmap::{DashMap, DashSet};
use futures::StreamExt;
use log::warn;
use tokio::fs::DirEntry;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;

use super::updater::FusedReadDir;
use super::File;

pub struct FileDatabase {
    pub(super) update: Option<SpawnHandle>,
    pub(super) data: FileDatabaseData,
}

impl Actor for FileDatabase {
    type Context = Context<Self>;
}

#[derive(Debug, Clone)]
pub struct FileDatabaseData {
    pub(super) paths: Arc<DashSet<PathBuf>>,
    database: Arc<DashMap<OsString, File>>,
    pub(super) queued_dirs: Arc<AtomicUsize>,
    pub(super) finished_dirs: Arc<AtomicUsize>,
    pub(super) updating: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl FileDatabaseData {
    pub(super) fn finished_dirs(&self) -> usize {
        self.finished_dirs.load(Ordering::Relaxed)
    }

    pub(super) fn finished_dirs_inc(&self) {
        self.finished_dirs.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn finished_dirs_reset(&self) {
        self.finished_dirs.store(0, Ordering::Relaxed);
    }

    pub(super) fn queued_dirs(&self) -> usize {
        self.queued_dirs.load(Ordering::Relaxed)
    }

    pub(super) fn queued_dirs_inc(&self) {
        self.queued_dirs.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn queued_dirs_reset(&self) {
        self.queued_dirs.store(0, Ordering::Relaxed);
    }

    pub(super) fn set_updating(&self, updating: bool) {
        self.updating.store(updating, Ordering::Relaxed);
    }

    pub(super) fn notify(&self) {
        self.notify.notify_waiters();
    }
}

pub trait FileDatabaseModel: Unpin + 'static {
    fn find_file(&self, name: &str) -> Option<File>;
    fn is_updating(&self) -> bool;
    fn queued_dirs(&self) -> usize;
    fn finished_dirs(&self) -> usize;
}

impl FileDatabaseModel for FileDatabaseData {
    fn find_file(&self, name: &str) -> Option<File> {
        let name = OsString::from_str(name).ok()?;
        self.database.get(&name).map(|p| p.value().clone())
    }

    fn is_updating(&self) -> bool {
        self.updating.load(Ordering::Relaxed)
    }

    fn queued_dirs(&self) -> usize {
        self.queued_dirs.load(Ordering::Relaxed)
    }

    fn finished_dirs(&self) -> usize {
        self.finished_dirs.load(Ordering::Relaxed)
    }
}

pub struct FileDatabaseUpdater {
    semaphore: Arc<Semaphore>,
    path: PathBuf,
    subdirs: JoinSet<Result<()>>,
    data: FileDatabaseData,
}

impl Drop for FileDatabaseUpdater {
    fn drop(&mut self) {
        self.subdirs.abort_all()
    }
}

impl FileDatabaseUpdater {
    pub fn new(path: PathBuf, data: FileDatabaseData) -> Self {
        FileDatabaseUpdater {
            path,
            data,
            subdirs: JoinSet::default(),
            semaphore: Arc::new(Semaphore::new(100)),
        }
    }

    fn clone_with(&self, path: PathBuf) -> Self {
        FileDatabaseUpdater {
            path,
            subdirs: Default::default(),
            data: self.data.clone(),
            semaphore: self.semaphore.clone(),
        }
    }

    #[async_recursion]
    pub async fn complete(mut self) -> Result<()> {
        self.data.queued_dirs_inc();

        self.crawl_dir().await?;
        self.finish_subdirs().await;

        self.data.finished_dirs_inc();
        self.data.notify();
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
            self.insert_file(entry.into())
        }
    }

    fn spawn_subdir_crawler(&mut self, path: PathBuf) {
        let subdir = self.clone_with(path).complete();
        self.subdirs.spawn(subdir);
    }

    fn insert_file(&self, file: File) {
        self.data.database.insert(file.name.clone(), file);
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
