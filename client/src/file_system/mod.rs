use std::ffi::OsString;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_recursion::async_recursion;
use dashmap::DashMap;
use futures::future::join_all;
use futures::StreamExt;
use iced::widget::{row, Button, Container, ProgressBar, Text, Tooltip};
use iced::{Element, Length, Renderer};
use log::{debug, warn};
use tokio::fs::DirEntry;
use tokio::sync::watch::{Receiver as WatchReceiver, Sender as WatchSender};
use tokio::sync::{Notify, RwLock, RwLockReadGuard, Semaphore};
use tokio::task::JoinSet;

use self::message::UpdateFinished;
use self::updater::FusedReadDir;
use crate::client::LogResult;
use crate::file_system::message::{Changed, DatabaseEvent};
use crate::iced_window::running::message::{StartDbUpdate, StopDbUpdate, UserEvent};
use crate::iced_window::MainMessage;
use crate::styling::{ContainerBorder, FileButton, FileProgressBar, ResultButton};
use crate::TEXT_SIZE;

pub mod actor;
pub mod control;
pub mod message;
pub mod updater;

#[cfg(test)]
pub mod file_database_test;

#[derive(Debug, Clone)]
pub struct File {
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct FileDatabase {
    sender: Arc<FileDatabaseProxy>,
    event_rx: WatchReceiver<DatabaseEvent>,
}

impl FileDatabase {
    pub fn new(path: &[PathBuf]) -> Self {
        let (event_tx, event_rx) = tokio::sync::watch::channel(Changed.into());
        let sender = Arc::new(FileDatabaseProxy::new(path, event_tx));
        Self { sender, event_rx }
    }

    pub fn sender(&self) -> &Arc<FileDatabaseProxy> {
        &self.sender
    }

    pub async fn recv(&mut self) -> Result<DatabaseEvent> {
        self.event_rx.changed().await?;
        let event = *self.event_rx.borrow().deref();
        Ok(event)
    }
}

#[derive(Debug)]
pub struct FileDatabaseUpdateData {
    semaphore: Arc<Semaphore>,
    database: DashMap<OsString, File>,
    queued_dirs: AtomicUsize,
    finished_dirs: AtomicUsize,
    event_tx: WatchSender<DatabaseEvent>,
}

impl FileDatabaseUpdateData {
    fn new(event_tx: WatchSender<DatabaseEvent>) -> Self {
        Self {
            event_tx,
            database: Default::default(),
            queued_dirs: AtomicUsize::new(0),
            finished_dirs: AtomicUsize::new(0),
            semaphore: Arc::new(Semaphore::new(100)),
        }
    }

    fn finished_dirs(&self) -> usize {
        self.finished_dirs.load(Ordering::Relaxed)
    }

    fn finished_dirs_inc(&self) {
        self.finished_dirs.fetch_add(1, Ordering::Relaxed);
    }

    fn finished_dirs_reset(&self) {
        self.finished_dirs.store(0, Ordering::Relaxed);
    }

    fn queued_dirs(&self) -> usize {
        self.queued_dirs.load(Ordering::Relaxed)
    }

    fn queued_dirs_inc(&self) {
        self.queued_dirs.fetch_add(1, Ordering::Relaxed);
    }

    fn queued_dirs_reset(&self) {
        self.queued_dirs.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct FileDatabaseProxy {
    stop: RwLock<Notify>,
    search_paths: RwLock<Vec<PathBuf>>,
    data: Arc<FileDatabaseUpdateData>,
}

impl FileDatabaseProxy {
    pub fn new(path: &[PathBuf], event_tx: WatchSender<DatabaseEvent>) -> Self {
        Self {
            search_paths: RwLock::new(path.to_vec()),
            stop: Default::default(),
            data: Arc::new(FileDatabaseUpdateData::new(event_tx)),
        }
    }

    pub fn start_update(db: &Arc<Self>) {
        let db = db.clone();
        tokio::spawn(async move { db.update().await });
    }

    pub async fn add_search_path(&self, path: PathBuf) {
        self.search_paths.write().await.push(path);
    }

    pub async fn clear_search_paths(&self) {
        self.search_paths.write().await.clear();
    }

    pub fn find_file(&self, name: &str) -> Result<Option<File>> {
        let name = OsString::from_str(name)?;
        Ok(self.data.database.get(&name).map(|p| p.value().clone()))
    }

    pub fn stop_update(&self) {
        self.stop.try_read().map(|s| s.notify_one()).ok();
    }

    pub fn get_status(&self) -> (usize, usize) {
        let finished = self.data.finished_dirs();
        let queued = self.data.queued_dirs();
        (finished, queued)
    }

    pub fn is_updating(&self) -> bool {
        self.stop.try_write().is_err()
    }

    pub fn view<'a>(&self) -> Element<'a, MainMessage, Renderer> {
        let (fin, que) = self.get_status();
        let finished = fin == que;
        let main: Element<_, _> = match finished {
            true => {
                let len = self.data.database.len();
                Container::new(
                    Button::new(Text::new(format!("{len} files in database")))
                        .style(FileButton::theme(false, true)),
                )
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(ContainerBorder::basic())
                .width(Length::Fill)
                .into()
            }
            false => ProgressBar::new(0.0..=(que as f32), fin as f32)
                .style(FileProgressBar::theme(fin == que))
                // Text size + 2 times default button padding
                .height(Length::Fixed(*TEXT_SIZE.load_full() + 10.0))
                .into(),
        };

        let update_msg = match finished {
            true => UserEvent::from(StartDbUpdate).into(),
            false => UserEvent::from(StopDbUpdate).into(),
        };
        let update_btn = match finished {
            true => Button::new("Update"),
            false => Button::new("Stop"),
        }
        .on_press(update_msg)
        .style(ResultButton::theme(finished));

        let update_text = match finished {
            true => "Update file database",
            false => "Stop update of file database",
        };
        let update_tooltip: Element<_, _> = Tooltip::new(
            update_btn,
            update_text,
            iced::widget::tooltip::Position::Bottom,
        )
        .into();
        row!(main, update_tooltip).spacing(5.0).into()
    }

    pub async fn update(&self) {
        let Ok(stop) = self.update_init() else {
            // If the init failed, we simply return.
            // We can do this because update_init()
            // can only fail if there is already an update in progress
            return;
        };
        self.update_paths(&stop).await;
        self.update_finish_up();
        drop(stop);
    }

    fn update_init(&self) -> Result<RwLockReadGuard<Notify>> {
        let mut stop = self.stop.try_write()?;
        *stop = Notify::new();

        self.data.database.clear();
        self.data.queued_dirs_reset();
        self.data.finished_dirs_reset();
        self.data.event_tx.send(Changed.into()).ok();

        Ok(stop.downgrade())
    }

    async fn update_paths<'a>(&self, stop: &RwLockReadGuard<'a, Notify>) {
        let mut paths = self.search_paths.read().await.clone();
        let futures = paths.drain(..).map(|p| self.update_path(p));
        tokio::select! {
            _ = join_all(futures) => { }
            _ = stop.notified() => debug!("update stop requested")
        }
    }

    fn update_finish_up(&self) {
        self.data.queued_dirs_reset();
        self.data.finished_dirs_reset();
        self.data.event_tx.send(UpdateFinished.into()).ok();
    }

    async fn update_path(&self, path: PathBuf) -> Result<()> {
        let updater = FileDatabaseUpdater::new(path, self.data.clone());
        updater.complete().await.log();
        Ok(())
    }
}

#[derive(Debug)]
struct FileDatabaseUpdater {
    path: PathBuf,
    subdirs: JoinSet<Result<()>>,
    data: Arc<FileDatabaseUpdateData>,
}

impl FileDatabaseUpdater {
    fn new(path: PathBuf, data: Arc<FileDatabaseUpdateData>) -> Self {
        FileDatabaseUpdater {
            path,
            data,
            subdirs: JoinSet::default(),
        }
    }

    fn clone_with(&self, path: PathBuf) -> Self {
        FileDatabaseUpdater {
            path,
            subdirs: Default::default(),
            data: self.data.clone(),
        }
    }

    #[async_recursion]
    async fn complete(mut self) -> Result<()> {
        self.data.queued_dirs_inc();

        self.crawl_dir().await?;
        self.finish_subdirs().await;

        self.data.finished_dirs_inc();
        self.data.event_tx.send(Changed.into())?;
        Ok(())
    }

    async fn crawl_dir(&mut self) -> Result<()> {
        let permit = self.data.semaphore.clone().acquire_owned();
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

impl From<DirEntry> for File {
    fn from(entry: DirEntry) -> Self {
        File {
            name: entry.file_name(),
            path: entry.path(),
        }
    }
}
