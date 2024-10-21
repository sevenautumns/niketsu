use std::collections::BTreeSet;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;
use im::Vector;
use itertools::Itertools;
use rayon::prelude::IntoParallelRefIterator;
use tokio::task::JoinHandle;
use tracing::{trace, warn};

use self::fuzzy::FuzzySearch;
use self::updater::FileDatabaseUpdater;
use super::player::MediaPlayerTrait;
use super::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use super::{CoreModel, EventHandler};

pub mod fuzzy;
mod updater;

const MAX_UPDATE_FREQUENCY: Duration = Duration::from_millis(100);

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait FileDatabaseTrait: std::fmt::Debug + Send {
    fn add_path(&mut self, path: PathBuf);
    fn del_path(&mut self, path: &Path);
    fn clear_paths(&mut self);
    fn get_paths(&self) -> Vec<PathBuf>;
    fn start_update(&mut self);
    fn stop_update(&mut self);
    fn find_file(&self, filename: &str) -> Option<FileEntry>;
    fn all_files(&self) -> &FileStore;
    async fn event(&mut self) -> Option<FileDatabaseEvent>;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum FileDatabaseEvent {
    UpdateComplete,
    UpdateProgress,
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateComplete;

impl From<UpdateComplete> for PlayerMessage {
    fn from(_: UpdateComplete) -> Self {
        PlayerMessageInner {
            message: "Filedatabase update complete".to_string(),
            source: MessageSource::Internal,
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into()
    }
}

impl EventHandler for UpdateComplete {
    fn handle(self, model: &mut CoreModel) {
        trace!("database update complete");
        let database = model.database.all_files();
        model.ui.file_database_status(1.0);
        model.ui.file_database(database.clone());
        model.ui.player_message(PlayerMessage::from(self));
        model.player.maybe_reload_video(database)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateProgress {
    pub ratio: f32,
}

impl EventHandler for UpdateProgress {
    fn handle(self, model: &mut CoreModel) {
        trace!("database update progress");
        model.ui.file_database_status(self.ratio);
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct FileEntry {
    inner: Arc<FileEntryInner>,
}

impl std::fmt::Debug for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl FileEntry {
    pub fn new(name: String, path: PathBuf, modified: Option<SystemTime>) -> Self {
        FileEntryInner::new(name, path, modified).into()
    }
}

impl Deref for FileEntry {
    type Target = FileEntryInner;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl From<FileEntryInner> for FileEntry {
    fn from(value: FileEntryInner) -> Self {
        let inner = Arc::new(value);
        Self { inner }
    }
}

#[derive(Clone, Eq)]
pub struct FileEntryInner {
    path: PathBuf,
    name: ArcStr,
    modified: Option<SystemTime>,
}

impl std::fmt::Debug for FileEntryInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileEntry")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("modified", &self.modified)
            .finish()
    }
}

impl std::hash::Hash for FileEntryInner {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.name.as_bytes())
    }
}

impl FileEntryInner {
    pub fn new(name: String, path: PathBuf, modified: Option<SystemTime>) -> Self {
        Self {
            path,
            name: name.into(),
            modified,
        }
    }

    pub fn file_name(&self) -> &str {
        &self.name
    }

    pub fn file_name_arc(&self) -> ArcStr {
        self.name.clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn modified(&self) -> Option<&SystemTime> {
        self.modified.as_ref()
    }
}

impl PartialEq for FileEntryInner {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

/// TODO rename after big merge to something which indicates that this only does the searching
#[derive(Debug, Default)]
pub struct FileDatabase {
    update: Option<JoinHandle<Vec<FileEntry>>>,
    progress: Arc<UpdateProgressTracker>,
    store: FileStore,
    paths: BTreeSet<PathBuf>,
    last_progress_event: Option<Instant>,
    stopped: bool,
}

impl FileDatabase {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let mut db = FileDatabase {
            paths: paths.into_iter().collect(),
            ..Default::default()
        };
        db.start_update();
        db
    }
}

#[derive(Debug, Default)]
struct UpdateProgressTracker {
    dirs_queued: AtomicUsize,
    dirs_finished: AtomicUsize,
}

impl UpdateProgressTracker {
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
        self.stopped = false;
    }

    fn stop_update(&mut self) {
        if let Some(update) = self.update.take() {
            self.progress = Arc::default();
            self.stopped = true;
            update.abort();
        }
    }

    fn find_file(&self, filename: &str) -> Option<FileEntry> {
        self.store.find_file(filename)
    }

    fn all_files(&self) -> &FileStore {
        &self.store
    }

    async fn event(&mut self) -> Option<FileDatabaseEvent> {
        // TODO REFACTOR
        if self.stopped {
            self.stopped = false;
            return Some(UpdateComplete.into());
        }

        use crate::file_database::UpdateProgress as Prog;
        let updater = self.update.as_mut()?;

        let Some(last) = self.last_progress_event else {
            self.last_progress_event = Some(Instant::now());
            return Some(
                Prog {
                    ratio: self.progress.percent_complete(),
                }
                .into(),
            );
        };
        let remaining_time = MAX_UPDATE_FREQUENCY.saturating_sub(last.elapsed());
        if remaining_time.is_zero() {
            self.last_progress_event = Some(Instant::now());
            return Some(
                Prog {
                    ratio: self.progress.percent_complete(),
                }
                .into(),
            );
        }
        tokio::select! {
            update = updater => {
                match update {
                    Ok(data) => self.store = FileStore::from_iter(data),
                    Err(error) => warn!(%error, "update error"),
                };
                self.update.take();
                Some(UpdateComplete.into())
            }
            _ = tokio::time::sleep(remaining_time) => {
                self.last_progress_event = Some(Instant::now());
                Some(Prog {
                    ratio: self.progress.percent_complete(),
                }
                .into())
            }
        }
    }
}

#[derive(Debug, Clone, Default, Eq)]
pub struct FileStore {
    store: Vector<FileEntry>,
}

impl PartialEq for FileStore {
    fn eq(&self, other: &Self) -> bool {
        self.store.eq(&other.store)
    }
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
        let mut store: Vector<_> = iter.into_iter().unique().collect();
        store.sort_by(|left, right| left.file_name().cmp(right.file_name()));
        Self { store }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use anyhow::Result;
    use tempfile::{tempdir, TempDir};
    use tokio::time::{sleep, Duration};

    use super::*;

    //TODO more test cases
    #[test]
    fn test_add_path() {
        let mut file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: Default::default(),
            last_progress_event: Default::default(),
            stopped: false,
        };
        let test_path = PathBuf::from("test/path/");
        file_db.add_path(test_path.clone());
        let expected = BTreeSet::from([test_path.clone()]);
        assert_eq!(expected, file_db.paths);

        let another_test_path = PathBuf::from("another/test/path/");
        file_db.add_path(another_test_path.clone());
        let expected = BTreeSet::from([test_path.clone(), another_test_path.clone()]);
        assert_eq!(expected, file_db.paths);
    }

    #[test]
    fn test_del_path() {
        let test_path = PathBuf::from("test/path/");
        let mut file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: BTreeSet::from([test_path.clone()]),
            last_progress_event: Default::default(),
            stopped: false,
        };
        file_db.del_path(Path::new("test/path"));
        let expected: BTreeSet<PathBuf> = Default::default();
        assert_eq!(expected, file_db.paths);
    }

    #[test]
    fn test_clear_path() {
        let mut file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: BTreeSet::from([PathBuf::from("test/path/"), PathBuf::from("test/path2/")]),
            last_progress_event: Default::default(),
            stopped: false,
        };
        file_db.clear_paths();
        let expected: BTreeSet<PathBuf> = Default::default();
        assert_eq!(expected, file_db.paths);
    }

    #[test]
    fn test_get_paths() {
        let paths: Vec<PathBuf> = vec!["test/path/".into(), "test/path2/".into()];
        let file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: paths.iter().cloned().collect(),
            last_progress_event: Default::default(),
            stopped: false,
        };
        let actual = file_db.get_paths();
        assert_eq!(paths, actual);
    }

    fn generate_test_dir(size: usize, fix: &str) -> Result<TempDir> {
        let tempdir = tempdir()?;
        let subdir = tempdir.path().join("TestFolder");
        std::fs::create_dir(&subdir)?;
        for i in 0..size {
            File::create(tempdir.path().join(format!("File_{i}_{fix}")))?;
            File::create(subdir.join(format!("{fix}_SubFile_{i}")))?;
        }
        Ok(tempdir)
    }

    #[tokio::test]
    async fn test_start_update() -> Result<()> {
        let dir = generate_test_dir(100, "fix")?;
        let dir2 = generate_test_dir(200, "fix")?;
        let dir3 = generate_test_dir(200, "suffix")?;
        let mut file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: BTreeSet::from([dir.into_path(), dir2.into_path(), dir3.into_path()]),
            last_progress_event: Default::default(),
            stopped: true,
        };
        file_db.start_update();
        let result = file_db.update.expect("failed to create join handle").await;
        assert_eq!(result.expect("failed to get results").len(), 1000);
        assert!(!file_db.stopped, "stop variable should not be set");
        Ok(())
    }

    #[tokio::test]
    async fn test_stop_update() {
        let mut file_db = FileDatabase {
            update: Default::default(),
            progress: Arc::new(UpdateProgressTracker {
                dirs_queued: AtomicUsize::new(0),
                dirs_finished: AtomicUsize::new(0),
            }),
            store: Default::default(),
            paths: Default::default(),
            last_progress_event: Default::default(),
            stopped: false,
        };
        file_db.update = Some(tokio::spawn(async move {
            sleep(Duration::from_secs(1)).await;
            Vec::new()
        }));
        file_db.stop_update();
        assert!(
            file_db.update.is_none(),
            "update handle should have stopped early"
        );
        assert!(file_db.stopped, "stop variable should be set");
    }

    // #[test]
    // fn test_find_file() {
    //     let mut file_db = FileDatabase {
    //         update: Default::default(),
    //         progress: Arc::new(UpdateProgress {
    //             dirs_queued: AtomicUsize::new(0),
    //             dirs_finished: AtomicUsize::new(0),
    //         }),
    //         store: Default::default(),
    //         paths: Default::default(),
    //         last_progress_event: Default::default(),
    //     };
    //     let mut file = file_db.find_file("some_file");
    //     assert!(file.is_none(), "non-existent file is found?");

    //     file_db.store = FileStore::from_iter(["test/path/file".into(), "test/path/file2".into()]);
    //     file = file_db.find_file("file");
    //     assert!(!file.is_none());
    //     assert_eq!(file.unwrap(), PathBuf::from("test/path/file"));
    // }

    // #[test]
    // fn test_all_files() {
    //     let mut file_db = FileDatabase {
    //         update: Default::default(),
    //         progress: Arc::new(UpdateProgress {
    //             dirs_queued: AtomicUsize::new(0),
    //             dirs_finished: AtomicUsize::new(0),
    //         }),
    //         store: Default::default(),
    //         paths: Default::default(),
    //         last_progress_event: Default::default(),
    //     };
    //     let mut files = file_db.all_files();
    //     assert_eq!(files.len(), 0);

    //     file_db.store = FileStore::from_iter(["test/path/file".into(), "test/path/file2".into()]);
    //     files = file_db.all_files();
    //     assert_eq!(files.len(), 2);
    //     assert_eq!(
    //         files.iter().cloned().collect::<HashSet<_>>(),
    //         HashSet::<(String, PathBuf)>::from([
    //             ("file".into(), "test/path/file".into()),
    //             ("file2".into(), "test/path/file2".into())
    //         ])
    //     );
    // }
}
