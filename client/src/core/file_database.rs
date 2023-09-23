use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use arcstr::ArcStr;
use async_trait::async_trait;
use chrono::Local;
use enum_dispatch::enum_dispatch;

use super::ui::{MessageLevel, MessageSource, PlayerMessage, PlayerMessageInner};
use super::{CoreModel, EventHandler};
use crate::file_database::FileStore;

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
        let database = model.database.all_files().clone();
        model.ui.file_database_status(1.0);
        model.ui.file_database(database);
        model.ui.player_message(PlayerMessage::from(self))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UpdateProgress {
    pub ratio: f32,
}

impl EventHandler for UpdateProgress {
    fn handle(self, model: &mut CoreModel) {
        model.ui.file_database_status(self.ratio);
    }
}

// pub trait FileDatabaseTraitExt {
//     fn upgrade_video(&self, video: PlaylistVideo) -> Option<PlayerVideo>;
// }

// impl<T: FileDatabaseTrait + ?Sized> FileDatabaseTraitExt for T {
//     fn upgrade_video(&self, video: PlaylistVideo) -> Option<PlayerVideo> {
//         match video {
//             PlaylistVideo::Url(url) => Some(PlayerVideo::Url(url)),
//             PlaylistVideo::File(name) => self.find_file(&name).map(PlayerVideo::File),
//         }
//     }
// }

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FileEntry {
    inner: Arc<FileEntryInner>,
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

#[derive(Debug, Clone, Eq)]
pub struct FileEntryInner {
    path: PathBuf,
    name: ArcStr,
    modified: Option<SystemTime>,
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
}

impl PartialEq for FileEntryInner {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}
