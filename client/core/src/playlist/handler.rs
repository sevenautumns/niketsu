use serde::{Deserialize, Serialize};

use crate::playlist::{Playlist, Video};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PlaylistHandler {
    playing: Option<usize>,
    #[serde(flatten)]
    playlist: Playlist,
}

impl PlaylistHandler {
    pub fn get_current_video(&self) -> Option<Video> {
        self.playlist.playlist.get(self.playing?).cloned()
    }

    /// The video after the one playing. Selecting it moves the marker, so
    /// the marker stays where the playing video is on every peer.
    pub fn next_video(&self) -> Option<Video> {
        self.playlist.playlist.get(self.playing? + 1).cloned()
    }

    /// Clears the marker when the video is not in the playlist, so every
    /// peer — including later joiners, who only get the room's state —
    /// agrees on "nothing selected" instead of some peers keeping a stale
    /// marker.
    pub fn select_playing(&mut self, video: &Video) {
        self.playing = self.playlist.find(video);
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
        self.playing = playing.and_then(|playing| self.playlist.find(&playing));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::Playlist;

    #[test]
    fn test_initial_state() {
        let handler = PlaylistHandler::default();
        // Ensure no video is playing initially.
        assert_eq!(handler.get_current_video(), None);
        // Ensure there is no next video when no video is playing.
        assert_eq!(handler.next_video(), None);
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
        // The next video is Video 2, and Video 1 stays marked.
        assert_eq!(handler.next_video(), Some(video2.clone()));
        assert_eq!(handler.get_current_video(), Some(video1.clone()));
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
    fn test_no_next_video_past_the_end() {
        let mut handler = PlaylistHandler::default();
        handler.replace(Playlist::from_iter(["Video 1"]));
        handler.select_playing(&Video::from("Video 1"));

        assert_eq!(handler.next_video(), None);
    }

    #[test]
    fn test_select_playing_missing_video_clears_marker() {
        let mut handler = PlaylistHandler::default();
        let video1 = Video::from("Video 1");
        handler.replace(Playlist::from_iter(["Video 1"]));

        handler.select_playing(&video1);
        assert_eq!(handler.get_current_video(), Some(video1));

        handler.select_playing(&Video::from("Video 2"));
        assert_eq!(handler.get_current_video(), None);
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
