use serde::{Deserialize, Serialize};

use crate::playlist::{Playlist, Video};

/// A playlist with its selected video, as a saved playlist file holds it.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PlaylistHandler {
    playing: Option<usize>,
    #[serde(flatten)]
    playlist: Playlist,
}

impl PlaylistHandler {
    /// `playlist` with `video` selected, if it is in the playlist.
    pub fn new(playlist: Playlist, video: Option<&Video>) -> Self {
        let playing = video.and_then(|video| playlist.find(video));
        Self { playing, playlist }
    }

    pub fn get_current_video(&self) -> Option<Video> {
        self.playlist.playlist.get(self.playing?).cloned()
    }

    pub fn get_playlist(&self) -> Playlist {
        self.playlist.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_selected_in_an_empty_playlist() {
        let handler = PlaylistHandler::default();
        assert_eq!(handler.get_current_video(), None);
    }

    #[test]
    fn a_video_of_the_playlist_is_selected() {
        let playlist = Playlist::from_iter(["Video 1", "Video 2"]);
        let video2 = Video::from("Video 2");

        let handler = PlaylistHandler::new(playlist.clone(), Some(&video2));

        assert_eq!(handler.get_current_video(), Some(video2));
        assert_eq!(handler.get_playlist(), playlist);
    }

    #[test]
    fn a_video_outside_the_playlist_is_not_selected() {
        let playlist = Playlist::from_iter(["Video 1"]);

        let handler = PlaylistHandler::new(playlist, Some(&Video::from("Video 2")));

        assert_eq!(handler.get_current_video(), None);
    }
}
