use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::fs::DirEntry;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

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
        Self::update_all_with(paths, progress, MAX_CONCURRENT_CRAWLER).await
    }

    /// Crawls `paths` with at most `crawlers` directories read at once.
    async fn update_all_with(
        paths: impl Iterator<Item = PathBuf>,
        progress: Arc<UpdateProgressTracker>,
        crawlers: usize,
    ) -> Vec<FileEntry> {
        let mut updater = JoinSet::default();
        let semaphore = Arc::new(Semaphore::new(crawlers));
        for path in paths {
            updater.spawn(
                Self::new(path.to_path_buf(), progress.clone(), semaphore.clone()).complete(),
            );
        }
        let mut database = Vec::new();
        while let Some(res) = updater.join_next().await {
            match res {
                Ok(Err(error)) => warn!(%error),
                Err(error) => warn!(%error),
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

    /// Holds a permit only while reading this directory. Its crawler waits
    /// for the subdirectories' crawlers afterwards, and holding the permit
    /// through that would let the parents of a deep tree take every permit
    /// while their children wait for one.
    async fn crawl_dir(&mut self) -> Result<()> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("the crawler semaphore closed")?;
        let mut read_dir = tokio::fs::read_dir(&self.path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
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
                Ok(Err(error)) => warn!(%error),
                Err(error) => warn!(%error),
                Ok(Ok(paths)) => self.paths.extend(paths),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;

    /// Writes an empty file at `path` below `root`, with its directories.
    fn touch(root: &Path, path: &Path) -> PathBuf {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"").unwrap();
        file
    }

    async fn crawl(root: &Path, crawlers: usize) -> BTreeSet<PathBuf> {
        let progress = Arc::new(UpdateProgressTracker::default());
        FileDatabaseUpdater::update_all_with([root.to_path_buf()].into_iter(), progress, crawlers)
            .await
            .into_iter()
            .map(|entry| entry.path().to_path_buf())
            .collect()
    }

    #[tokio::test]
    async fn finds_every_file_of_a_nested_tree() {
        let root = TempDir::new().unwrap();
        let files: BTreeSet<_> = ["top.mkv", "a/one.mkv", "a/b/two.mkv", "a/b/c/three.mkv"]
            .into_iter()
            .chain(["d/four.mkv", "d/e/five.mkv"])
            .map(|path| touch(root.path(), Path::new(path)))
            .collect();
        std::fs::create_dir_all(root.path().join("empty/dir")).unwrap();

        assert_eq!(crawl(root.path(), MAX_CONCURRENT_CRAWLER).await, files);
    }

    /// A tree deeper and wider than there are crawlers completes: a crawler
    /// waiting for its subdirectories holds no permit.
    #[tokio::test]
    async fn a_tree_beyond_the_crawler_limit_completes() {
        let root = TempDir::new().unwrap();
        let mut files = BTreeSet::new();
        let mut deep = PathBuf::new();
        for depth in 0..MAX_CONCURRENT_CRAWLER + 20 {
            deep.push("d");
            files.insert(touch(root.path(), &deep.join(format!("{depth}.mkv"))));
        }
        for width in 0..MAX_CONCURRENT_CRAWLER + 20 {
            files.insert(touch(
                root.path(),
                &Path::new(&format!("w{width}")).join("f.mkv"),
            ));
        }

        let crawled = tokio::time::timeout(
            Duration::from_secs(30),
            crawl(root.path(), MAX_CONCURRENT_CRAWLER),
        )
        .await
        .expect("the crawl deadlocked");
        assert_eq!(crawled, files);
    }

    #[tokio::test]
    async fn a_single_crawler_reads_a_whole_tree() {
        let root = TempDir::new().unwrap();
        let files: BTreeSet<_> = [
            "a/b/c/one.mkv",
            "a/b/two.mkv",
            "x/y/three.mkv",
            "x/four.mkv",
        ]
        .into_iter()
        .map(|path| touch(root.path(), Path::new(path)))
        .collect();

        let crawled = tokio::time::timeout(Duration::from_secs(10), crawl(root.path(), 1))
            .await
            .expect("the crawl deadlocked");
        assert_eq!(crawled, files);
    }

    /// Reading a directory waits for a permit.
    #[tokio::test]
    async fn without_a_permit_nothing_is_read() {
        let root = TempDir::new().unwrap();
        touch(root.path(), Path::new("one.mkv"));

        let crawled = tokio::time::timeout(Duration::from_millis(200), crawl(root.path(), 0)).await;
        assert!(crawled.is_err(), "a directory was read without a permit");
    }
}
