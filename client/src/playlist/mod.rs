use std::ops::{Range, RangeBounds};

use arcstr::ArcStr;
use im::Vector;
use itertools::Itertools;

use crate::core::playlist::*;

#[derive(Debug, Default)]
pub struct PlaylistHandler {
    playing: Option<usize>,
    playlist: Playlist,
}

impl PlaylistHandler {
    pub fn get_current_video(&self) -> Option<PlaylistVideo> {
        self.playlist.list.get(self.playing?).cloned()
    }

    pub fn advance_to_next(&mut self) -> Option<PlaylistVideo> {
        if let Some(playing) = self.playing.as_mut() {
            *playing += 1
        }
        self.get_current_video()
    }

    pub fn select_playing(&mut self, video: &PlaylistVideo) {
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

    pub fn append(&mut self, video: PlaylistVideo) {
        self.playlist.append(video)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Playlist {
    list: Vector<PlaylistVideo>,
}

impl Playlist {
    pub fn iter(&self) -> PlaylistIter<'_> {
        self.into_iter()
    }

    pub fn find(&self, video: &PlaylistVideo) -> Option<usize> {
        self.list
            .iter()
            .enumerate()
            .find(|(_, v)| v.eq(&video))
            .map(|(i, _)| i)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&PlaylistVideo> {
        self.list.get(index)
    }

    pub fn get_range(&self, from: usize, to: usize) -> impl Iterator<Item = &PlaylistVideo> {
        self.list.iter().skip(from).take(to - from + 1)
    }

    pub fn move_video(&mut self, video: &PlaylistVideo, index: usize) {
        let mut new_index = index;
        if let Some(old_index) = self.find(video) {
            let video = self.list.remove(old_index);
            new_index = new_index.min(self.list.len());
            self.list.insert(new_index, video);
        } else {
            new_index = new_index.min(self.list.len());
            self.list.insert(new_index, video.clone());
        }
    }

    pub fn move_range(&mut self, range: Range<usize>, mut target_index: usize) {
        let slice = self.list.slice(range.clone());

        let target_adjustment = target_index
            .saturating_sub(range.start)
            .min(range.end - range.start);
        target_index -= target_adjustment;

        let rest = self.list.split_off(target_index);
        self.list.append(slice);
        self.list.append(rest);
    }

    pub fn move_indices(&mut self, indices: &[usize], mut target_index: usize) {
        let indices = indices.iter().unique().sorted().copied();
        let mut moved = Vector::default();
        for i in indices.enumerate().map(|(offset, i)| i - offset) {
            moved.push_back(self.list.remove(i));
            if target_index > i {
                target_index -= 1;
            }
        }
        let target_index = target_index.min(self.list.len());
        let rest = self.list.split_off(target_index);
        self.list.append(moved);
        self.list.append(rest);
    }

    pub fn remove(&mut self, index: usize) {
        if self.list.get(index).is_some() {
            self.list.remove(index);
        }
    }

    pub fn remove_by_video(&mut self, video: &PlaylistVideo) -> Option<PlaylistVideo> {
        if let Some(index) = self.find(video) {
            return Some(self.list.remove(index));
        }
        None
    }

    pub fn append(&mut self, video: PlaylistVideo) {
        if !self.contains(&video) {
            self.list.push_back(video);
        }
    }

    pub fn insert(&mut self, index: usize, video: PlaylistVideo) {
        if !self.contains(&video) {
            self.list.insert(index, video)
        }
    }

    pub fn remove_range<R: RangeBounds<usize>>(&mut self, range: R) {
        self.list.slice(range);
    }

    pub fn contains(&mut self, video: &PlaylistVideo) -> bool {
        self.list.contains(video)
    }
}

impl<'a> FromIterator<&'a str> for Playlist {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let list = iter.into_iter().map(PlaylistVideo::from).collect();
        Self { list }
    }
}

impl<'a> FromIterator<&'a ArcStr> for Playlist {
    fn from_iter<T: IntoIterator<Item = &'a ArcStr>>(iter: T) -> Self {
        let list = iter.into_iter().map(PlaylistVideo::from).collect();
        Self { list }
    }
}

impl<'a> IntoIterator for &'a Playlist {
    type Item = &'a PlaylistVideo;

    type IntoIter = PlaylistIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PlaylistIter {
            iter: self.list.iter(),
        }
    }
}

pub struct PlaylistIter<'a> {
    iter: im::vector::Iter<'a, PlaylistVideo>,
}

impl<'a> Iterator for PlaylistIter<'a> {
    type Item = &'a PlaylistVideo;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let mut handler = PlaylistHandler::default();
        // Ensure no video is playing initially.
        assert_eq!(handler.get_current_video(), None);
        // Ensure advancing to the next video when no video is playing returns None.
        assert_eq!(handler.advance_to_next(), None);
    }

    #[test]
    fn test_append() {
        let mut handler = PlaylistHandler::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        assert_eq!(handler.get_playlist().len(), 0);

        // Append two videos and verify their presence.
        handler.append(video1.clone());
        handler.select_playing(&video1);
        handler.append(video2.clone());
        assert_eq!(handler.get_current_video(), Some(&video1).cloned());
        assert_eq!(handler.get_playlist().len(), 2);
        assert_eq!(handler.get_playlist().get(0), Some(&video1));
        assert_eq!(handler.get_playlist().get(1), Some(&video2));
    }

    #[test]
    fn test_select_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        handler.append(video1.clone());
        handler.append(video2.clone());

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
        let video2 = PlaylistVideo::from("Video 2");

        let new_playlist = Playlist::from_iter(["Video 2"]);
        handler.replace(new_playlist.clone());
        // Ensure the playlist is replaced and contains Video 2.
        assert_eq!(handler.get_playlist().get(0), Some(&video2));
    }

    #[test]
    fn test_unload_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = PlaylistVideo::from("Video 1");

        handler.append(video1.clone());
        assert_eq!(handler.get_current_video(), None);

        handler.select_playing(&video1);
        assert_eq!(handler.get_current_video(), Some(&video1).cloned());

        // Unload the currently playing video and ensure it's None.
        handler.unload_playing();
        assert_eq!(handler.get_current_video(), None);
    }

    #[test]
    fn test_initial_playlist_state() {
        let playlist = Playlist::default();
        assert_eq!(playlist.len(), 0);
        assert!(playlist.is_empty());
        assert_eq!(playlist.get(0), None);
    }

    #[test]
    fn test_append_to_playlist() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        playlist.append(video1.clone());
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video1));

        playlist.append(video2.clone());
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(1), Some(&video2));
    }

    #[test]
    fn test_remove_from_playlist() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        assert_eq!(playlist.len(), 2);

        playlist.remove(0);
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video2));
    }

    #[test]
    fn test_remove_by_video() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        assert_eq!(playlist.len(), 2);

        playlist.remove_by_video(&video1);
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video2));
    }

    #[test]
    fn test_move_video_within_playlist() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());

        // Move video2 to the beginning
        playlist.move_video(&video2, 0);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video3));
    }

    #[test]
    fn test_move_video_within_playlist_back() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Move video2 to the beginning
        playlist.move_video(&video2, 3);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video4));
        assert_eq!(playlist.get(3), Some(&video2));
        assert_eq!(playlist.get(4), Some(&video5));
    }

    #[test]
    fn test_move_video() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());

        // Move Video 2 to the beginning.
        playlist.move_video(&video2, 0);

        // Verify the order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video3));
    }

    #[test]
    fn test_remove_range() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Remove the range from index 1 to 3 (inclusive).
        playlist.remove_range(1..=3);

        // Verify the resulting playlist.
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video5));
    }

    #[test]
    fn test_edge_cases() {
        let mut empty_playlist = Playlist::default();
        // Removing from an empty playlist should not panic.
        empty_playlist.remove(0);

        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        // Moving in an empty playlist should not panic.
        playlist.move_video(&video1, 0);
    }

    #[test]
    fn test_iteration() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());

        let mut iter = playlist.iter();

        assert_eq!(iter.next(), Some(&video1));
        assert_eq!(iter.next(), Some(&video2));
        assert_eq!(iter.next(), Some(&video3));
        assert_eq!(iter.next(), None);
    }

    // Test cloning the playlist.
    #[test]
    fn test_cloning() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        playlist.append(video1.clone());
        playlist.append(video2.clone());

        let cloned_playlist = playlist.clone();

        // Modify the original playlist.
        playlist.remove(0);

        // Verify that the cloned playlist remains unchanged.
        assert_eq!(cloned_playlist.len(), 2);
        assert_eq!(cloned_playlist.get(0), Some(&video1));
        assert_eq!(cloned_playlist.get(1), Some(&video2));
    }

    #[test]
    fn test_replace_with_currently_playing() {
        let mut handler = PlaylistHandler::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        handler.append(video1.clone());
        handler.append(video2.clone());
        handler.append(video3.clone());

        // Select Video 2 to be playing.
        handler.select_playing(&video2);

        // Replace the playlist with a new one containing Video 2.
        let new_playlist = Playlist::from_iter(["Video 2"]);
        handler.replace(new_playlist.clone());

        // Verify that the currently playing video is still Video 2.
        assert_eq!(handler.get_current_video(), Some(video2.clone()));
    }

    #[test]
    fn test_move_video_new_index_greater_than_old_index() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());

        // Move Video 2 to a new index greater than its old index.
        playlist.move_video(&video2, 2);

        // Verify that Video 2 and 3 are in the correct position.
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video2));
    }

    #[test]
    fn test_insert_into_playlist() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");

        playlist.append(video1.clone());
        playlist.append(video3.clone());

        // Insert Video 2 at index 1.
        playlist.insert(1, video2.clone());

        // Verify the resulting playlist.
        assert_eq!(playlist.len(), 3);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
    }

    #[test]
    fn test_playlist_from_iter_arcstr() {
        let arcstr1 = ArcStr::from("ArcStr 1");
        let arcstr2 = ArcStr::from("ArcStr 2");
        let arcstr3 = ArcStr::from("ArcStr 3");

        let playlist: Playlist = vec![&arcstr1, &arcstr2, &arcstr3].into_iter().collect();

        // Verify that the playlist contains the expected videos.
        assert_eq!(playlist.len(), 3);
        assert_eq!(playlist.get(0), Some(&PlaylistVideo::from(&arcstr1)));
        assert_eq!(playlist.get(1), Some(&PlaylistVideo::from(&arcstr2)));
        assert_eq!(playlist.get(2), Some(&PlaylistVideo::from(&arcstr3)));
    }

    #[test]
    fn test_move_first_element_to_right_and_back() {
        let mut playlist = Playlist::default();

        // Initialize the playlist with 5 elements.
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Verify the initial order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        // Move the first element (Video 1) one by one to the right.
        playlist.move_video(&video1, 1);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 2);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video1));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 3);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video4));
        assert_eq!(playlist.get(3), Some(&video1));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 4);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video4));
        assert_eq!(playlist.get(3), Some(&video5));
        assert_eq!(playlist.get(4), Some(&video1));

        // Move the first element (Video 1) one by one back to the left.
        playlist.move_video(&video1, 3);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video4));
        assert_eq!(playlist.get(3), Some(&video1));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 2);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video1));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 1);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_video(&video1, 0);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));
    }

    #[test]
    fn test_move_first_two_elements_together() {
        let mut playlist = Playlist::default();

        // Initialize the playlist with 5 elements.
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Verify the initial order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        // Move the first two elements together one step after another to the right.
        playlist.move_range(0..2, 3); // Move Video 1 and Video 2 to index 4.
        assert_eq!(playlist.get(0), Some(&video3));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video2));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_range(1..3, 4); // Move Video 1 and Video 2 to index 4.
        assert_eq!(playlist.get(0), Some(&video3));
        assert_eq!(playlist.get(1), Some(&video4));
        assert_eq!(playlist.get(2), Some(&video1));
        assert_eq!(playlist.get(3), Some(&video2));
        assert_eq!(playlist.get(4), Some(&video5));

        // Move the first two elements together one step after another back to the front.
        playlist.move_range(2..4, 1); // Move Video 1 and Video 2 to index 1.
        assert_eq!(playlist.get(0), Some(&video3));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video2));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        playlist.move_range(1..3, 0); // Move Video 1 and Video 2 to index 0.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));
    }

    #[test]
    fn test_move_all() {
        let mut playlist = Playlist::default();

        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        playlist.append(video1.clone());
        playlist.append(video2.clone());

        // Verify the initial order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));

        playlist.move_range(0..2, 1);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
    }

    #[test]
    fn test_move_indices() {
        let mut playlist = Playlist::default();

        // Initialize the playlist with 5 elements.
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Verify the initial order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        // Define the indices to be moved.
        let indices_to_move = vec![1, 3];

        // Move the specified indices to index 0.
        playlist.move_indices(&indices_to_move, 0);

        // Verify the new order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video4));
        assert_eq!(playlist.get(2), Some(&video1));
        assert_eq!(playlist.get(3), Some(&video3));
        assert_eq!(playlist.get(4), Some(&video5));
    }

    #[test]
    fn test_move_indices_to_back() {
        let mut playlist = Playlist::default();

        // Initialize the playlist with 5 elements.
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");
        let video3 = PlaylistVideo::from("Video 3");
        let video4 = PlaylistVideo::from("Video 4");
        let video5 = PlaylistVideo::from("Video 5");

        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video3.clone());
        playlist.append(video4.clone());
        playlist.append(video5.clone());

        // Verify the initial order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
        assert_eq!(playlist.get(3), Some(&video4));
        assert_eq!(playlist.get(4), Some(&video5));

        // Define the indices to be moved to the back.
        let indices_to_move = vec![0, 2, 4];

        // Move the specified indices to the back.
        playlist.move_indices(&indices_to_move, playlist.len());

        // Verify the new order of videos in the playlist.
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video4));
        assert_eq!(playlist.get(2), Some(&video1));
        assert_eq!(playlist.get(3), Some(&video3));
        assert_eq!(playlist.get(4), Some(&video5));
    }

    #[test]
    fn test_no_duplicate_elements() {
        let mut playlist = Playlist::default();
        let video1 = PlaylistVideo::from("Video 1");
        let video2 = PlaylistVideo::from("Video 2");

        // Append Video 1 twice to create a duplicate.
        playlist.append(video1.clone());
        playlist.append(video2.clone());
        playlist.append(video1.clone());

        // Verify that the duplicate Video 1 is not in the playlist.
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));

        // Attempt to insert Video 2 again at index 0.
        playlist.insert(0, video2.clone());

        // Verify that the duplicate Video 1 is not inserted.
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
    }
}
