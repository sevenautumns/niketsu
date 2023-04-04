use anyhow::{bail, Result};
use arc_swap::ArcSwapOption;
use std::sync::Arc;
use std::{collections::HashMap, ffi::OsString, path::PathBuf};
use tokio::sync::watch::{Receiver as WatchReceiver, Sender as WatchSender};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use async_recursion::async_recursion;

#[derive(Debug)]
pub struct File {
    pub name: OsString,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct FileDatabase {
    stop_tx: ArcSwapOption<WatchSender<bool>>,
    update_lock: Mutex<()>,
    // update_id: <Uuid>,
    database: RwLock<HashMap<OsString, File>>,
    search_paths: RwLock<Vec<PathBuf>>,
}

// TODO
// - combine async requests
impl FileDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn database(&self) -> &RwLock<HashMap<OsString, File>> {
        &self.database
    }

    pub async fn add_search_path(&self, path: PathBuf) {
        self.search_paths.write().await.push(path);
    }

    pub fn stop_update(&self) {
        // TODO wait for stop
        // TODO also make sure we do not wait on a potential next update
        if let Some(tx) = self.stop_tx.swap(None) {
            tx.send(true).unwrap();
        }
    }

    pub async fn update(&self) -> Result<()> {
        let update_lock = match self.update_lock.try_lock() {
            Ok(lock) => lock,
            Err(_) => bail!("Already update in progress"),
        };
        // self.update_id.store(Some(Arc::new(Uuid::new_v4())));

        let (tx, rx) = tokio::sync::watch::channel(false);
        self.stop_tx.store(Some(Arc::new(tx)));

        self.database.write().await.clear();
        let paths = self.search_paths.read().await.clone();
        for p in paths {
            if self.update_inner(p.to_path_buf(), rx.clone()).await.is_ok() {
                // for f in files {
                //     self.database.insert(f.name.clone(), f);
                // }
            }
        }

        self.stop_tx.store(None);
        // self.update_id.store(None);
        drop(update_lock);
        Ok(())
    }

    #[async_recursion]
    async fn update_inner(&self, path: PathBuf, stop: WatchReceiver<bool>) -> Result<()> {
        let mut files = vec![];
        let mut dir = tokio::fs::read_dir(path).await?;
        while let Some(entry) = dir.next_entry().await? {
            // Early exit
            if *stop.borrow() {
                return Ok(());
            }

            let typ = entry.file_type().await?;
            if typ.is_dir() {
                if self.update_inner(entry.path(), stop.clone()).await.is_ok() {
                    // for f in inner {
                    //     out.push(f)
                    // }
                }
            } else if typ.is_file() {
                files.push(File {
                    name: entry.file_name(),
                    path: entry.path(),
                })
            }
            // TODO follow symlink?
        }
        let mut lock = self.database.write().await;
        for f in files {
            lock.insert(f.name.clone(), f);
        }

        Ok(())
        // Ok(out)
    }
}
