use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use im::Vector;
use log::warn;
use rayon::prelude::IntoParallelRefIterator;
use tokio::task::JoinHandle;

use self::fuzzy::FuzzySearch;
use self::updater::FileDatabaseUpdater;
use crate::core::file_database::{FileDatabaseEvent, FileDatabaseTrait, FileEntry, UpdateComplete};

pub mod fuzzy;
mod updater;

const MAX_UPDATE_FREQUENCY: Duration = Duration::from_millis(100);

/// TODO rename after big merge to something which indicates that this only does the searching
#[derive(Debug, Default)]
pub struct FileDatabase {
    update: Option<JoinHandle<Vec<FileEntry>>>,
    progress: Arc<UpdateProgress>,
    store: FileStore,
    paths: BTreeSet<PathBuf>,
    last_progress_event: Option<Instant>,
}

#[derive(Debug, Default)]
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
        let paths = self.paths.clone().into_iter();
        let progress = self.progress.clone();
        let update = FileDatabaseUpdater::update_all(paths, progress);
        self.last_progress_event = None;
        self.update = Some(tokio::task::spawn(update));
    }

    fn stop_update(&mut self) {
        if let Some(update) = &mut self.update {
            self.progress = Arc::default();
            update.abort();
        }
    }

    fn find_file(&self, filename: &str) -> Option<FileEntry> {
        self.store.find_file(filename)
    }

    fn all_files(&self) -> &FileStore {
        &self.store
    }

    async fn event(&mut self) -> FileDatabaseEvent {
        // TODO refactor
        use crate::core::file_database::UpdateProgress as Prog;
        let Some(updater) = self.update.as_mut() else {
            tokio::time::sleep(Duration::from_secs(6000)).await;
            return Prog { ratio: 1.0 }.into();
        };

        let Some(last) = self.last_progress_event else {
            self.last_progress_event = Some(Instant::now());
            return Prog {
                ratio: self.progress.percent_complete(),
            }
            .into();
        };
        let remaining_time = MAX_UPDATE_FREQUENCY.saturating_sub(last.elapsed());
        if remaining_time.is_zero() {
            self.last_progress_event = Some(Instant::now());
            return Prog {
                ratio: self.progress.percent_complete(),
            }
            .into();
        }
        tokio::select! {
            update = updater => {
                match update {
                    Ok(data) => self.store = FileStore::from_iter(data),
                    Err(e) => warn!("Update error: {e:?}"),
                };
                self.update.take();
                UpdateComplete.into()
            }
            _ = tokio::time::sleep(remaining_time) => {
                self.last_progress_event = Some(Instant::now());
                Prog {
                    ratio: self.progress.percent_complete(),
                }
                .into()
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileStore {
    store: Vector<FileEntry>,
}

impl FileStore {
    pub fn len(&self) -> usize {
        self.store.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn find_file(&self, filename: &str) -> Option<FileEntry> {
        let index = self
            .store
            .binary_search_by(|f| f.file_name().cmp(filename))
            .ok()?;
        self.store.get(index).cloned()
    }

    pub fn iter(&self) -> im::vector::Iter<'_, FileEntry> {
        self.into_iter()
    }

    pub fn fuzzy_search(&self, query: String) -> FuzzySearch {
        FuzzySearch::new(query, self.clone())
    }
}

impl<'a> IntoIterator for &'a FileStore {
    type Item = &'a FileEntry;

    type IntoIter = im::vector::Iter<'a, FileEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.store.iter()
    }
}

impl<'a> IntoParallelRefIterator<'a> for FileStore {
    type Iter = im::vector::rayon::ParIter<'a, FileEntry>;

    type Item = &'a FileEntry;

    fn par_iter(&'a self) -> Self::Iter {
        self.store.par_iter()
    }
}

impl FromIterator<FileEntry> for FileStore {
    fn from_iter<T: IntoIterator<Item = FileEntry>>(iter: T) -> Self {
        let mut store: Vector<_> = iter.into_iter().collect();
        store.sort_by(|left, right| left.file_name().cmp(right.file_name()));
        Self { store }
    }
}
