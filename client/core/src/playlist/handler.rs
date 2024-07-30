use serde::{Deserialize, Serialize};

use crate::playlist::{Playlist, Video};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlaylistHandler {
    playing: Option<usize>,
    #[serde(flatten)]
    playlist: Playlist,
}

impl PlaylistHandler {
    pub fn get_current_video(&self) -> Option<Video> {
        self.playlist.playlist.get(self.playing?).cloned()
    }

    pub fn advance_to_next(&mut self) -> Option<Video> {
        if let Some(playing) = self.playing.as_mut() {
            *playing += 1
        }
        self.get_current_video()
    }

    pub fn select_playing(&mut self, video: &Video) {
        if let Some(index) = self.playlist.find(video) {
            self.playing = Some(index);
        }
    }

    pub fn unload_playing(&mut self) {
        self.playing = None
    }

    pub fn get_playlist(&self) -> Playlist {
        self.playlist.clone()
    }

    pub fn replace(&mut self, playlist: Playlist) {
        let playing = self.get_current_video();
        self.playlist = playlist;
        if let Some(playing) = playing {
            self.playing = self.playlist.find(&playing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::Playlist;

    #[test]
    fn test_initial_state() {
        let mut handler = PlaylistHandler::default();
        // Ensure no video is playing initially.
        assert_eq!(handler.get_current_video(), None);
        // Ensure advancing to the next video when no video is playing returns None.
        assert_eq!(handler.advance_to_next(), None);
    }

    #[test]
    fn test_select_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let mut playlist = Playlist::default();
        playlist.push(video1.clone());
        playlist.push(video2.clone());
        handler.replace(playlist);

        // Select the first video to play and verify it's playing.
        handler.select_playing(&video1);
        assert_eq!(handler.get_current_video(), Some(video1.clone()));
        // Advance to the next video and ensure it's Video 2.
        assert_eq!(handler.advance_to_next(), Some(video2.clone()));
        // Verify the current video is Video 2.
        assert_eq!(handler.get_current_video(), Some(video2.clone()));
    }

    #[test]
    fn test_replace_playlist() {
        let mut handler = PlaylistHandler::default();
        let video2 = Video::from("Video 2");

        let new_playlist = Playlist::from_iter(["Video 2"]);
        handler.replace(new_playlist.clone());
        // Ensure the playlist is replaced and contains Video 2.
        assert_eq!(handler.get_playlist().get(0), Some(&video2));
    }

    #[test]
    fn test_unload_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = Video::from("Video 1");
        let mut playlist = Playlist::default();
        playlist.push(video1.clone());

        handler.replace(playlist);
        assert_eq!(handler.get_current_video(), None);

        handler.select_playing(&video1);
        assert_eq!(handler.get_current_video(), Some(&video1).cloned());

        // Unload the currently playing video and ensure it's None.
        handler.unload_playing();
        assert_eq!(handler.get_current_video(), None);
    }

    #[test]
    fn test_replace_with_currently_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        let mut playlist = Playlist::default();
        playlist.push(video1);
        playlist.push(video2.clone());
        playlist.push(video3);
        handler.replace(playlist);

        // Select Video 2 to be playing.
        handler.select_playing(&video2);

        // Replace the playlist with a new one containing Video 2.
        let new_playlist = Playlist::from_iter(["Video 2"]);
        handler.replace(new_playlist.clone());

        // Verify that the currently playing video is still Video 2.
        assert_eq!(handler.get_current_video(), Some(video2.clone()));
    }
}
