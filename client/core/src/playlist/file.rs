use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};

use chrono::Local;
use once_cell::sync::Lazy;
use tokio::sync::Semaphore;

use super::handler::PlaylistHandler;
use crate::room::RoomName;
use crate::PROJECT_DIRS;

static PLAYLIST_FOLDER: Lazy<Option<PathBuf>> =
    Lazy::new(|| PROJECT_DIRS.as_ref().map(|p| p.data_dir().join("playlist")));

static TIMESTAMP: Lazy<String> = Lazy::new(|| Local::now().format("%Y-%m-%d_%H%M%S").to_string());

static SAVE_PERMIT: Semaphore = Semaphore::const_new(1);

const EXTENSION: &str = "yaml";

pub struct PlaylistBrowser {}

impl PlaylistBrowser {
    fn get_playlist_folder() -> Option<&'static PathBuf> {
        let playlist = PLAYLIST_FOLDER.as_ref();
        if playlist.is_none() {
            tracing::error!("failed to get playlist folder")
        }
        playlist
    }

    async fn get_playlist_from_path(path: &Path) -> Option<PlaylistHandler> {
        let playlist = tokio::fs::read_to_string(path)
            .await
            .inspect_err(|error| tracing::warn!(file = ?path, %error, "failed reading file"))
            .ok()?;
        serde_yaml::from_str(&playlist)
            .inspect_err(
                |error| tracing::warn!(playlist = ?path, %error, "failed parsing playlist"),
            )
            .ok()
    }

    pub async fn get_first(room: &RoomName) -> Option<PlaylistHandler> {
        let mut paths = Self::get_all_paths_for_room(room).await;
        paths.sort_by_cached_key(|path| path.file_name().map(OsStr::to_os_string));
        for path in paths.iter().rev() {
            if let playlist @ Some(_) = Self::get_playlist_from_path(path).await {
                return playlist;
            }
        }
        tracing::warn!("No playlist found");
        None
    }

    pub async fn get_all_paths_for_room(room: &RoomName) -> Vec<PathBuf> {
        let mut names = vec![];
        let Some(playlist_folder) = Self::get_playlist_folder() else {
            return vec![];
        };
        let room_folder = playlist_folder.join(room.as_str());
        let mut read_dir = match tokio::fs::read_dir(&room_folder).await {
            Ok(read_dir) => read_dir,
            Err(error) => {
                tracing::error!(?room_folder, %error, "failed to read folder");
                return vec![];
            }
        };
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let file_type = entry.file_type().await;
            if !file_type.as_ref().is_ok_and(FileType::is_file) {
                continue;
            }
            if entry.path().extension().is_some_and(|e| e.eq(EXTENSION)) {
                names.push(entry.path());
            }
        }
        names
    }

    pub async fn get_all_for_room(room: &RoomName) -> Vec<PlaylistHandler> {
        let mut paths = Self::get_all_paths_for_room(room).await;
        paths.sort_by_cached_key(|path| path.file_name().map(OsStr::to_os_string));
        let mut handlers = Vec::with_capacity(paths.len());
        for path in paths.iter().rev() {
            if let Some(handler) = Self::get_playlist_from_path(path).await {
                handlers.push(handler);
            }
        }
        handlers.shrink_to_fit();
        handlers
    }

    pub async fn get_all() -> BTreeMap<RoomName, Vec<PlaylistHandler>> {
        let mut rooms = BTreeMap::new();
        let Some(playlist_folder) = Self::get_playlist_folder() else {
            return BTreeMap::new();
        };
        let mut read_dir = match tokio::fs::read_dir(playlist_folder).await {
            Ok(read_dir) => read_dir,
            Err(error) => {
                tracing::warn!(%error, ?playlist_folder, "failed to read folder");
                return BTreeMap::new();
            }
        };
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let file_type = entry.file_type().await;
            if !file_type.as_ref().is_ok_and(FileType::is_dir) {
                continue;
            }
            let Some(room) = entry
                .path()
                .file_name()
                .and_then(OsStr::to_str)
                .map(RoomName::from)
            else {
                continue;
            };
            let playlists = Self::get_all_for_room(&room).await;
            if !playlists.is_empty() {
                rooms.insert(room, playlists);
            }
        }
        rooms
    }

    pub(crate) fn save(room: &RoomName, handler: &PlaylistHandler) {
        let Ok(playlist) = serde_yaml::to_string(handler)
            .inspect_err(|error| tracing::error!(%error, "failed to serialize the playlist"))
        else {
            return;
        };
        let Some(playlist_folder) = Self::get_playlist_folder() else {
            return;
        };
        let mut filepath = playlist_folder.join(room.as_str()).join(TIMESTAMP.as_str());
        filepath.set_extension(EXTENSION);

        tokio::task::spawn(async move {
            let permit = SAVE_PERMIT.acquire().await;
            if let Some(parent) = filepath.parent() {
                if let Err(error) = tokio::fs::create_dir_all(parent).await {
                    tracing::error!(%error, "error creating directories");
                };
            }
            if let Err(error) = tokio::fs::write(filepath, playlist).await {
                tracing::error!(%error, "error saving playlist");
            };
            drop(permit);
        });
    }
}
