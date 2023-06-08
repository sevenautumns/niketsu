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
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use iced::widget::{row, Button, Container, ProgressBar, Text, Tooltip};
use iced::{Element, Length, Renderer};
use log::trace;
use tokio::sync::watch::{Receiver as WatchReceiver, Sender as WatchSender};
use tokio::sync::{Notify, RwLock, Semaphore};

use self::message::UpdateFinished;
use crate::client::database::message::{Changed, DatabaseEvent};
use crate::iced_window::running::message::{StartDbUpdate, StopDbUpdate, UserEvent};
use crate::iced_window::MainMessage;
use crate::styling::{ContainerBorder, FileButton, FileProgressBar, ResultButton};
use crate::TEXT_SIZE;

pub mod message;

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
pub struct FileDatabaseSender {
    semaphore: Semaphore,
    stop: RwLock<Notify>,
    database: DashMap<OsString, File>,
    search_paths: RwLock<Vec<PathBuf>>,
    queued_dirs: AtomicUsize,
    finished_dirs: AtomicUsize,
    event_tx: WatchSender<DatabaseEvent>,
}

impl FileDatabaseSender {
    pub fn new(path: &[PathBuf], event_tx: WatchSender<DatabaseEvent>) -> Self {
        Self {
            search_paths: RwLock::new(path.to_vec()),
            stop: Default::default(),
            database: Default::default(),
            semaphore: Semaphore::new(100),
            queued_dirs: AtomicUsize::new(0),
            finished_dirs: AtomicUsize::new(0),
            event_tx,
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
        Ok(self.database.get(&name).map(|p| p.value().clone()))
    }

    pub fn stop_update(&self) {
        self.stop.try_read().map(|s| s.notify_one()).ok();
    }

    pub fn get_status(&self) -> (usize, usize) {
        let finished = self.finished_dirs.load(Ordering::Relaxed);
        let queued = self.queued_dirs.load(Ordering::Relaxed);
        (finished, queued)
    }

    pub fn is_updating(&self) -> bool {
        self.stop.try_write().is_ok()
    }

    pub fn view<'a>(&self) -> Element<'a, MainMessage, Renderer> {
        let (fin, que) = self.get_status();
        let finished = fin == que;
        let main: Element<_, _> = match finished {
            true => {
                let len = self.database.len();
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
        let mut stop = match self.stop.try_write() {
            Ok(lock) => lock,
            Err(_) => return,
        };

        *stop = Notify::new();
        self.database.clear();
        self.queued_dirs.store(0, Ordering::Relaxed);
        self.finished_dirs.store(0, Ordering::Relaxed);
        self.event_tx.send(Changed.into()).ok();
        let paths = self.search_paths.read().await.clone();
        let stop = stop.downgrade();

        let mut join = vec![];
        for p in paths {
            join.push(self.update_inner(p.to_path_buf()));
        }

        tokio::select! {
            _ = join_all(join) => { }
            _ = stop.notified() => trace!("update stop requested")
        }

        self.queued_dirs.store(0, Ordering::Relaxed);
        self.finished_dirs.store(0, Ordering::Relaxed);
        self.event_tx.send(UpdateFinished.into()).ok();
        drop(stop);
    }

    #[async_recursion]
    async fn update_inner(&self, path: PathBuf) -> Result<()> {
        self.queued_dirs.fetch_add(1, Ordering::Relaxed);
        let sem = self.semaphore.acquire().await;
        let mut files = vec![];
        let mut join_rec = FuturesUnordered::new();
        let mut join_type = FuturesUnordered::new();
        let mut dir = tokio::fs::read_dir(path).await?;

        loop {
            tokio::select! {
                entry = dir.next_entry() => {
                    match entry? {
                        Some(entry) => {
                            join_type.push(async move {
                                (entry.file_name(), entry.path(), entry.file_type().await)
                            });
                        },
                        None => break,
                    }
                },
                Some((name, path, typ)) = join_type.next() => {
                    if let Ok(typ) = typ {
                        if typ.is_dir() {
                            join_rec.push(self.update_inner(path))
                        } else if typ.is_file() {
                            files.push(File {
                                name,
                                path,
                            })
                        }
                    }
                }
                Some(_) = join_rec.next() => {}
            }
        }
        loop {
            tokio::select! {
                res = join_type.next() => {
                    if let Some((name, path, typ)) = res {
                        if let Ok(typ) = typ {
                            if typ.is_dir() {
                                join_rec.push(self.update_inner(path))
                            } else if typ.is_file() {
                                files.push(File {
                                    name,
                                    path,
                                })
                            }
                        }
                    } else {
                        drop(sem);
                        break
                    }
                }
                Some(_) = join_rec.next() => {}
            }
        }
        while join_rec.next().await.is_some() {}

        for f in files {
            self.database.insert(f.name.clone(), f);
        }

        self.finished_dirs.fetch_add(1, Ordering::Relaxed);
        self.event_tx.send(Changed.into())?;
        Ok(())
    }
}
