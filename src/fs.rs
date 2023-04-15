use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_recursion::async_recursion;
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use iced::{Command, Subscription};
use log::trace;
use tokio::sync::watch::{Receiver as WatchRec, Sender as WatchSend};
use tokio::sync::{Notify, RwLock, Semaphore};

use crate::window::MainMessage;

#[derive(Debug, Clone)]
pub struct File {
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum DatabaseMessage {
    Changed,
    UpdateFinished(Arc<Result<()>>),
    FindFinished(Arc<Result<Option<File>>>),
}

#[derive(Debug)]
pub struct FileDatabase {
    stop: RwLock<Notify>,
    database: RwLock<HashMap<OsString, File>>,
    database_counter: (WatchSend<usize>, WatchRec<usize>),
    search_paths: RwLock<Vec<PathBuf>>,
    semaphore: Semaphore,
}

impl Default for FileDatabase {
    fn default() -> Self {
        Self {
            stop: Default::default(),
            database: Default::default(),
            database_counter: tokio::sync::watch::channel(0),
            search_paths: Default::default(),
            semaphore: Semaphore::new(100),
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

    pub fn subscription(&self) -> Subscription<MainMessage> {
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

    pub fn database(&self) -> &RwLock<HashMap<OsString, File>> {
        &self.database
    }

    pub async fn add_search_path(&self, path: PathBuf) {
        self.search_paths.write().await.push(path);
    }

    pub async fn clear_search_paths(&self) {
        self.search_paths.write().await.clear();
    }

    pub fn find_command(db: &Arc<Self>, name: &str) -> Command<MainMessage> {
        async fn find(db: Arc<FileDatabase>, name: String) -> MainMessage {
            MainMessage::Database(DatabaseMessage::FindFinished(Arc::new(
                db.find_file(&name).await,
            )))
        }
        Command::single(iced_native::command::Action::Future(
            find(db.clone(), name.to_string()).boxed(),
        ))
    }

    pub async fn find_file(&self, name: &str) -> Result<Option<File>> {
        let name = OsString::from_str(name)?;
        Ok(self.database.read().await.get(&name).cloned())
    }

    pub fn stop_update(&self) {
        self.stop.try_read().map(|s| s.notify_one()).ok();
    }

    pub fn database_counter(&self) -> usize {
        *self.database_counter.1.borrow()
    }

    pub async fn update(&self) -> Result<()> {
        let mut stop = match self.stop.try_write() {
            Ok(lock) => lock,
            Err(_) => bail!("Update or stop already in progress"),
        };

        *stop = Notify::new();
        self.database.write().await.clear();
        self.database_counter.0.send_modify(|i| *i += 1);
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

        drop(stop);
        Ok(())
    }

    #[async_recursion]
    async fn update_inner(&self, path: PathBuf) -> Result<()> {
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

        let mut lock = self.database.write().await;
        for f in files {
            lock.insert(f.name.clone(), f);
        }
        self.database_counter.0.send_modify(|i| *i += 1);

        Ok(())
    }
}
