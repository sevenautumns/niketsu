use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};

use chrono::Local;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use once_cell::sync::Lazy;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;
use tokio::sync::Semaphore;

use super::handler::PlaylistHandler;
use crate::room::RoomName;
use crate::util::FuzzyResult;
use crate::PROJECT_DIRS;

static PLAYLIST_FOLDER: Lazy<Option<PathBuf>> =
    Lazy::new(|| PROJECT_DIRS.as_ref().map(|p| p.data_dir().join("playlist")));

static TIMESTAMP: Lazy<String> = Lazy::new(|| Local::now().format("%Y-%m-%d_%H%M%S").to_string());

static SAVE_PERMIT: Semaphore = Semaphore::const_new(1);

const EXTENSION: &str = "yaml";

#[derive(Default, Debug, Clone)]
pub struct PlaylistBrowser {
    playlist_map: BTreeMap<RoomName, Vec<NamedPlaylist>>,
}

impl PlaylistBrowser {
    pub fn playlist_map(&self) -> &BTreeMap<RoomName, Vec<NamedPlaylist>> {
        &self.playlist_map
    }

    /// Fuzzy search over all playlist with the name combination: room name + playlist name
    /// The indices in the fuzzy result will be aligned to the name of the playlist without the room name
    pub fn fuzzy_search(&self, query: &str) -> Vec<FuzzyResult<NamedPlaylist>> {
        let matcher = SkimMatcherV2::default();

        let mut lists = self
            .playlist_map
            .par_iter()
            .map(|(_, list)| list)
            .flatten()
            .filter_map(|playlist| {
                matcher
                    .fuzzy_indices(&format!("{}/{}", playlist.room, playlist.name), query)
                    .map(|(score, hits)| FuzzyResult {
                        score,
                        hits: hits
                            .into_iter()
                            .filter_map(|i| i.checked_sub(playlist.room.len()))
                            .collect(),
                        entry: playlist.clone(),
                    })
            })
            .collect::<Vec<_>>();
        lists.par_sort_by_key(|r| -r.score);
        lists
    }

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
            .inspect_err(|error| tracing::warn!(?path, %error, "failed reading file"))
            .ok()?;
        serde_yaml::from_str(&playlist)
            .inspect_err(|error| tracing::warn!(?path, %error, "failed parsing playlist"))
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
        tracing::warn!(%room, "No playlist found");
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

    pub async fn get_all_for_room(room: &RoomName) -> Vec<NamedPlaylist> {
        let mut paths = Self::get_all_paths_for_room(room).await;
        paths.sort_by_cached_key(|path| path.file_name().map(OsStr::to_os_string));
        let mut playlists = Vec::with_capacity(paths.len());
        for path in paths.iter().rev() {
            if let Some(playlist) = Self::get_playlist_from_path(path).await {
                let name = path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| room.to_string());
                playlists.push(NamedPlaylist {
                    name,
                    room: room.clone(),
                    playlist,
                });
            }
        }
        playlists.shrink_to_fit();
        playlists
    }

    pub async fn get_all() -> PlaylistBrowser {
        let mut playlist_map = BTreeMap::new();
        let Some(playlist_folder) = Self::get_playlist_folder() else {
            return PlaylistBrowser::default();
        };
        let mut read_dir = match tokio::fs::read_dir(playlist_folder).await {
            Ok(read_dir) => read_dir,
            Err(error) => {
                tracing::warn!(%error, ?playlist_folder, "failed to read folder");
                return PlaylistBrowser::default();
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
                playlist_map.insert(room, playlists);
            }
        }
        PlaylistBrowser { playlist_map }
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

#[derive(Debug, Clone)]
pub struct NamedPlaylist {
    pub name: String,
    pub room: RoomName,
    pub playlist: PlaylistHandler,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_search() {
        // Set up sample playlists
        let mut playlist_browser = PlaylistBrowser::default();

        let room = arcstr::literal!("Room 1");
        let playlists = vec![
            NamedPlaylist {
                name: "Chill Vibes".to_string(),
                room: room.clone(),
                playlist: PlaylistHandler::default(),
            },
            NamedPlaylist {
                name: "Upbeat Hits".to_string(),
                room: room.clone(),
                playlist: PlaylistHandler::default(),
            },
            NamedPlaylist {
                name: "Chill Beats".to_string(),
                room: room.clone(),
                playlist: PlaylistHandler::default(),
            },
        ];

        playlist_browser
            .playlist_map
            .insert(room.clone(), playlists);

        // Test cases
        let results = playlist_browser.fuzzy_search("Chill");
        assert_eq!(results.len(), 2); // "Chill Vibes" and "Chill Beats"

        let first_result = &results[0];
        assert!(first_result.entry.name.contains("Chill"));
        assert!(first_result.score > 0);

        let results_no_match = playlist_browser.fuzzy_search("Party");
        assert_eq!(results_no_match.len(), 0); // No matches for "Party"

        let results_partial_match = playlist_browser.fuzzy_search("Vibes");
        assert_eq!(results_partial_match.len(), 1); // Only "Chill Vibes" should match
        assert_eq!(results_partial_match[0].entry.name, "Chill Vibes");
    }
}
