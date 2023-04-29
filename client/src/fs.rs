use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_recursion::async_recursion;
use atomic_counter::{AtomicCounter, RelaxedCounter};
use dashmap::DashMap;
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use iced::widget::{row, Button, Container, ProgressBar, Text, Tooltip};
use iced::{Command, Element, Length, Renderer, Subscription};
use log::trace;
use tokio::sync::watch::{Receiver as WatchRec, Sender as WatchSend};
use tokio::sync::{Notify, RwLock, Semaphore};

use crate::styling::{ContainerBorder, FileButton, FileProgressBar, ResultButton};
use crate::window::MainMessage;
use crate::TEXT_SIZE;

#[derive(Debug, Clone)]
pub struct File {
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum DatabaseMessage {
    Changed,
    UpdateFinished(Arc<Result<()>>),
}

#[derive(Debug)]
pub struct FileDatabase {
    stop: RwLock<Notify>,
    database: Arc<DashMap<OsString, File>>,
    database_counter: (WatchSend<usize>, WatchRec<usize>),
    search_paths: RwLock<Vec<PathBuf>>,
    semaphore: Semaphore,
    queued_dirs: RelaxedCounter,
    finished_dirs: RelaxedCounter,
}

impl Default for FileDatabase {
    fn default() -> Self {
        Self {
            stop: Default::default(),
            database: Default::default(),
            database_counter: tokio::sync::watch::channel(0),
            search_paths: Default::default(),
            semaphore: Semaphore::new(100),
            queued_dirs: RelaxedCounter::new(0),
            finished_dirs: RelaxedCounter::new(0),
        }
    }
}

impl FileDatabase {
    pub fn new(path: &[PathBuf]) -> Self {
        Self {
            search_paths: RwLock::new(path.to_vec()),
            ..Default::default()
        }
    }

    pub fn subscribe(&self) -> Subscription<MainMessage> {
        iced::subscription::unfold(
            std::any::TypeId::of::<Self>(),
            self.database_counter.1.clone(),
            |mut dc| async move {
                // TODO do something in error case?
                dc.changed().await.ok();
                (MainMessage::Database(DatabaseMessage::Changed), dc)
            },
        )
    }

    pub fn update_command(db: &Arc<Self>) -> Command<MainMessage> {
        async fn update(db: Arc<FileDatabase>) -> MainMessage {
            MainMessage::Database(DatabaseMessage::UpdateFinished(Arc::new(db.update().await)))
        }
        Command::single(iced_native::command::Action::Future(
            update(db.clone()).boxed(),
        ))
    }

    // pub fn database(&self) -> Arc<DashMap<OsString, File>> {
    //     self.database.clone()
    // }

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
        let finished = self.finished_dirs.get();
        let queued = self.queued_dirs.get();
        (finished, queued)
    }

    pub fn is_updating(&self) -> bool {
        self.stop.try_write().is_ok()
    }

    pub fn database_counter(&self) -> usize {
        *self.database_counter.1.borrow()
    }

    pub fn view<'a>(&self) -> Element<'a, MainMessage, Renderer> {
        let (fin, que) = self.get_status();
        let finished = fin == que;
        let main: Element<_, _> = match finished {
            true => {
                let len = self.database.len();
                Container::new(
                    Button::new(Text::new(format!("{len} files in database")))
                        .style(FileButton::new(false, true)),
                )
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .style(ContainerBorder::basic())
                .width(Length::Fill)
                .into()
            }
            false => ProgressBar::new(0.0..=(que as f32), fin as f32)
                .style(FileProgressBar::new(fin == que))
                // Text size + 2 times default button padding
                .height(Length::Fixed(*TEXT_SIZE.load_full().unwrap() + 10.0))
                .into(),
        };

        let update_msg = match finished {
            true => MainMessage::User(crate::window::UserMessage::StartDbUpdate),
            false => MainMessage::User(crate::window::UserMessage::StopDbUpdate),
        };
        let update_btn = match finished {
            true => Button::new("Update"),
            false => Button::new("Stop"),
        }
        .on_press(update_msg)
        .style(ResultButton::new(finished));

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

    pub async fn update(&self) -> Result<()> {
        let mut stop = match self.stop.try_write() {
            Ok(lock) => lock,
            Err(_) => bail!("Update or stop already in progress"),
        };

        *stop = Notify::new();
        self.database.clear();
        self.database_counter.0.send_modify(|i| *i += 1);
        self.queued_dirs.reset();
        self.finished_dirs.reset();
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

        self.queued_dirs.reset();
        self.finished_dirs.reset();
        drop(stop);
        Ok(())
    }

    #[async_recursion]
    async fn update_inner(&self, path: PathBuf) -> Result<()> {
        self.queued_dirs.inc();
        let sem = self.semaphore.acquire().await;
        let mut files = vec![];
        let mut join_rec = FuturesUnordered::new();
        let mut join_type = FuturesUnordered::new();
        let mut dir = tokio::fs::read_dir(path).await.unwrap();

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
        self.database_counter.0.send_modify(|i| *i += 1);

        self.finished_dirs.inc();
        Ok(())
    }
}
