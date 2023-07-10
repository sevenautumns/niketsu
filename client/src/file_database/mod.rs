use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use log::warn;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use self::updater::FileDatabaseUpdater;
use crate::core::file_database::FileDatabaseTrait;

mod updater;

#[derive(Debug)]
pub struct FileDatabase {
    update: Option<JoinHandle<HashMap<String, PathBuf>>>,
    progress: Arc<UpdateProgress>,
    data: HashMap<String, PathBuf>,
    paths: BTreeSet<PathBuf>,
    completed: Notify,
}

#[derive(Debug)]
struct UpdateProgress {
    dirs_queued: AtomicUsize,
    dirs_finished: AtomicUsize,
}

impl UpdateProgress {
    fn inc_queued(&self) {
        self.dirs_queued.fetch_add(1, Ordering::Relaxed);
    }

    fn inc_finished(&self) {
        self.dirs_finished.fetch_add(1, Ordering::Relaxed);
    }

    fn percent_complete(&self) -> f32 {
        let finished = self.dirs_finished.load(Ordering::Relaxed);
        let queued = self.dirs_queued.load(Ordering::Relaxed);
        if finished >= queued {
            return 1.0;
        }
        finished as f32 / queued as f32
    }
}

#[async_trait]
impl FileDatabaseTrait for FileDatabase {
    fn add_path(&mut self, path: PathBuf) {
        self.paths.insert(path);
    }

    fn del_path(&mut self, path: &Path) {
        self.paths.remove(path);
    }

    fn clear_paths(&mut self) {
        self.paths.clear();
    }

    fn get_paths(&self) -> Vec<PathBuf> {
        self.paths.iter().cloned().collect()
    }

    fn start_update(&mut self) {
        if self.update.is_some() {
            warn!("update already in progress");
            return;
        }
        let update =
            FileDatabaseUpdater::update_all(self.paths.clone().into_iter(), self.progress.clone());
        self.update = Some(tokio::task::spawn(update));
    }

    fn stop_update(&mut self) {
        let Some(update) = self.update.take() else {
            return;
        };
        update.abort();
        self.completed.notify_waiters();
    }

    fn update_status(&self) -> f32 {
        self.progress.percent_complete()
    }

    fn find_file(&self, filename: &str) -> Option<PathBuf> {
        self.data.get(filename).cloned()
    }

    fn all_files(&self) -> Vec<PathBuf> {
        self.data.iter().map(|p| p.1.clone()).collect()
    }

    async fn update_completed(&mut self) {
        self.completed.notified().await
    }
}
