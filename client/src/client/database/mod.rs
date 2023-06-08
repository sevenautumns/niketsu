use std::ffi::OsString;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use dashmap::DashMap;
use enum_dispatch::enum_dispatch;
use futures::future::join_all;
use futures::stream::FusedStream;
use futures::StreamExt;
use iced::widget::{row, Button, Container, ProgressBar, Text, Tooltip};
use iced::{Element, Length, Renderer};
use log::{debug, warn};
use tokio::fs::{DirEntry, ReadDir};
use tokio::sync::watch::{Receiver as WatchReceiver, Sender as WatchSender};
use tokio::sync::{Notify, OwnedSemaphorePermit, RwLock, RwLockReadGuard, Semaphore};
use tokio::task::JoinSet;

use self::message::UpdateFinished;
use self::updater::FusedReadDir;
use super::LogResult;
use crate::client::database::message::{Changed, DatabaseEvent};
use crate::iced_window::running::message::{StartDbUpdate, StopDbUpdate, UserEvent};
use crate::iced_window::MainMessage;
use crate::styling::{ContainerBorder, FileButton, FileProgressBar, ResultButton};
use crate::TEXT_SIZE;

pub mod message;
pub mod updater;

#[derive(Debug, Clone)]
pub struct File {
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct FileDatabaseReceiver {
    sender: Arc<FileDatabaseSender>,
    event_rx: WatchReceiver<DatabaseEvent>,
}

impl FileDatabaseReceiver {
    pub fn new(path: &[PathBuf]) -> Self {
        let (event_tx, event_rx) = tokio::sync::watch::channel(Changed.into());
        let sender = Arc::new(FileDatabaseSender::new(path, event_tx));
        Self { sender, event_rx }
    }

    pub fn sender(&self) -> &Arc<FileDatabaseSender> {
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
pub struct FileDatabaseSender {
    stop: RwLock<Notify>,
    search_paths: RwLock<Vec<PathBuf>>,
    data: Arc<FileDatabaseUpdateData>,
}

impl FileDatabaseSender {
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

#[derive(Debug, Clone)]
struct FileDatabaseUpdater {
    path: PathBuf,
    data: Arc<FileDatabaseUpdateData>,
}

impl FileDatabaseUpdater {
    fn new(path: PathBuf, data: Arc<FileDatabaseUpdateData>) -> Self {
        FileDatabaseUpdater { path, data }
    }

    fn new_from_this(&self, path: PathBuf) -> Self {
        FileDatabaseUpdater {
            path,
            ..self.clone()
        }
    }

    async fn complete(self) -> Result<()> {
        self.data.queued_dirs_inc();

        let mut crawler = DirectoryCrawler::new(self.path, self.data.clone()).await?;
        crawler.complete().await?;

        self.data.finished_dirs_inc();
        self.data.event_tx.send(Changed.into())?;
        Ok(())
    }
}

#[derive(Debug)]
struct DirectoryCrawler {
    data: Arc<FileDatabaseUpdateData>,
    permit: Option<OwnedSemaphorePermit>,
    dir: FusedReadDir,
    typ_tasks: JoinSet<Result<Item>>,
    rec_tasks: JoinSet<Result<()>>,
}

impl Drop for DirectoryCrawler {
    fn drop(&mut self) {
        self.typ_tasks.abort_all();
        self.rec_tasks.abort_all();
    }
}

impl DirectoryCrawler {
    async fn new(path: PathBuf, data: Arc<FileDatabaseUpdateData>) -> Result<Self> {
        Ok(Self {
            permit: None,
            dir: FusedReadDir::new(path).await?,
            typ_tasks: JoinSet::new(),
            rec_tasks: JoinSet::new(),
            data,
        })
    }

    fn completed(&self) -> bool {
        self.completed_locally() && self.rec_tasks.is_empty()
    }

    fn completed_locally(&self) -> bool {
        self.dir.is_terminated() && self.typ_tasks.is_empty()
    }

    async fn complete(&mut self) -> Result<()> {
        while !self.completed() {
            self.next().await?
        }
        Ok(())
    }

    async fn next(&mut self) -> Result<()> {
        self.check_permit().await?;
        tokio::select! {
            Some(entry) = self.dir.next() => {
                self.typ_tasks.spawn(Item::from_dir_entry(entry));
            }
            Some(Ok(entry)) = self.typ_tasks.join_next() => self.handle_entry(entry),
            Some(Ok(res)) = self.rec_tasks.join_next() => self.handle_rec_res(res),
        }
        Ok(())
    }

    /// Drop permit if this crawler finished locally
    /// Acquire permit if it is not finished and no permit exists
    async fn check_permit(&mut self) -> Result<()> {
        if self.completed_locally() && self.permit.is_some() {
            drop(self.permit.take());
        }

        if !self.completed_locally() && self.permit.is_none() {
            self.permit = Some(self.data.semaphore.clone().acquire_owned().await?);
        }

        Ok(())
    }

    fn handle_entry(&mut self, entry: Result<Item>) {
        let Ok(entry) = entry else {
            warn!("{entry:?}");
            return;
        };
        entry.handle(self)
    }

    fn handle_rec_res(&self, res: Result<()>) {
        if let Err(e) = res {
            warn!("{e:?}")
        }
    }
}

#[enum_dispatch(CrawlerItem)]
#[derive(Debug)]
enum Item {
    File,
    Directory,
}

impl Item {
    async fn from_dir_entry(dir_entry: DirEntry) -> Result<Self> {
        let typ = dir_entry.file_type().await?;
        let path = dir_entry.path();
        if typ.is_symlink() {
            bail!("symlinks are not supported: {path:?}")
        }
        if typ.is_dir() {
            return Ok(Directory::new(path).into());
        }
        let name = dir_entry.file_name();
        Ok(File::new(name, path).into())
    }
}

#[enum_dispatch]
trait CrawlerItem {
    fn handle(self, crawler: &mut DirectoryCrawler);
}

#[derive(Debug)]
struct Directory {
    path: PathBuf,
}

impl Directory {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl CrawlerItem for Directory {
    fn handle(self, crawler: &mut DirectoryCrawler) {
        let updater = crawler.new_from_this(self.path);
        crawler.rec_tasks.spawn(updater.complete());
    }
}

impl File {
    pub fn new(name: OsString, path: PathBuf) -> Self {
        Self { name, path }
    }
}

impl CrawlerItem for File {
    fn handle(self, crawler: &mut DirectoryCrawler) {
        crawler.data.database.insert(self.name.clone(), self);
    }
}
