use std::ops::{Deref, Range, RangeBounds};
use std::slice::{Iter, SliceIndex};
use std::sync::Arc;

use arcstr::ArcStr;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::FilePathSearch;

pub mod file;
pub mod handler;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Video {
    #[serde(flatten)]
    inner: Arc<VideoInner>,
}

impl std::fmt::Debug for Video {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Deref for Video {
    type Target = VideoInner;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl From<VideoInner> for Video {
    fn from(value: VideoInner) -> Self {
        let inner = Arc::new(value);
        Self { inner }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoInner {
    File(ArcStr),
    Url(Arc<Url>),
}

impl VideoInner {
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    pub fn to_path_str(&self, f: &dyn FilePathSearch) -> Option<String> {
        match self {
            VideoInner::File(name) => f.get_file_path(name),
            VideoInner::Url(url) => Some(url.as_str().to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            VideoInner::File(name) => name,
            VideoInner::Url(url) => url.as_str(),
        }
    }
}

impl AsRef<str> for Video {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for Video {
    fn from(value: &str) -> Self {
        VideoInner::from(value).into()
    }
}

impl From<&str> for VideoInner {
    fn from(value: &str) -> Self {
        if let Ok(url) = Url::parse(value) {
            Self::Url(Arc::new(url))
        } else {
            Self::File(value.into())
        }
    }
}

impl From<&ArcStr> for Video {
    fn from(value: &ArcStr) -> Self {
        VideoInner::from(value).into()
    }
}

impl From<&ArcStr> for VideoInner {
    fn from(value: &ArcStr) -> Self {
        if let Ok(url) = Url::parse(value) {
            Self::Url(Arc::new(url))
        } else {
            Self::File(value.clone())
        }
    }
}

// TODO implement diffing with https://crates.io/crates/comparable
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    playlist: Vec<Video>,
}

impl Playlist {
    pub fn iter(&self) -> Iter<'_, Video> {
        self.playlist.iter()
    }

    pub fn find(&self, video: &Video) -> Option<usize> {
        self.playlist
            .iter()
            .enumerate()
            .find(|(_, v)| v.eq(&video))
            .map(|(i, _)| i)
    }

    pub fn len(&self) -> usize {
        self.playlist.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.playlist.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Video> {
        self.playlist.get(index)
    }

    pub fn get_range(&self, from: usize, to: usize) -> impl Iterator<Item = &Video> {
        self.playlist.iter().skip(from).take(to - from + 1)
    }

    pub fn move_video(&mut self, video: &Video, index: usize) {
        let mut new_index = index;
        if let Some(old_index) = self.find(video) {
            let video = self.playlist.remove(old_index);
            new_index = new_index.min(self.playlist.len());
            self.playlist.insert(new_index, video);
        } else {
            new_index = new_index.min(self.playlist.len());
            self.playlist.insert(new_index, video.clone());
        }
    }

    pub fn move_range(&mut self, range: Range<usize>, mut target_index: usize) {
        let slice = self.playlist.drain(range.clone()).collect_vec();

        let target_adjustment = target_index
            .saturating_sub(range.start)
            .min(range.end - range.start);
        target_index -= target_adjustment;

        let rest = self.playlist.split_off(target_index);
        self.playlist.extend(slice);
        self.playlist.extend(rest);
    }

    pub fn move_indices(&mut self, indices: &[usize], mut target_index: usize) {
        let indices = indices.iter().unique().sorted().copied();
        let mut moved = Vec::default();
        for i in indices.enumerate().map(|(offset, i)| i - offset) {
            moved.push(self.playlist.remove(i));
            if target_index > i {
                target_index -= 1;
            }
        }
        let target_index = target_index.min(self.playlist.len());
        let rest = self.playlist.split_off(target_index);
        self.playlist.extend(moved);
        self.playlist.extend(rest);
    }

    pub fn remove(&mut self, index: usize) {
        if self.playlist.get(index).is_some() {
            self.playlist.remove(index);
        }
    }

    pub fn remove_by_video(&mut self, video: &Video) -> Option<Video> {
        if let Some(index) = self.find(video) {
            return Some(self.playlist.remove(index));
        }
        None
    }

    pub fn push(&mut self, video: Video) {
        if !self.contains(&video) {
            self.playlist.push(video);
        }
    }

    pub fn append(&mut self, videos: impl Iterator<Item = Video>) {
        self.playlist.extend(videos.collect_vec())
    }

    pub fn insert(&mut self, index: usize, video: Video) {
        if !self.contains(&video) {
            self.playlist.insert(index, video)
        }
    }

    pub fn insert_range(&mut self, index: usize, videos: Vec<Video>) {
        for video in videos {
            self.insert(index, video);
        }
    }

    pub fn append_at(&mut self, index: usize, videos: impl Iterator<Item = Video>) {
        let rest = self.playlist.split_off(index);
        let videos = videos.filter(|v| !self.contains(v)).collect_vec();
        self.playlist.extend(videos);
        self.playlist.extend(rest);
    }

    pub fn remove_range<R: RangeBounds<usize>>(&mut self, range: R) -> Vec<Video> {
        self.playlist.drain(range).collect_vec()
    }

    pub fn reverse_range<R: SliceIndex<[Video], Output = [Video]>>(&mut self, range: R) {
        self.playlist[range].reverse();
    }

    pub fn contains(&mut self, video: &Video) -> bool {
        self.playlist.contains(video)
    }
}

impl<'a> FromIterator<&'a str> for Playlist {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let list = iter.into_iter().map(Video::from).collect();
        Self { playlist: list }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::file_database::FileEntry;
    use crate::FileStore;

    #[test]
    fn test_is_url() {
        let file_video = Video::from("video.mp4");
        let url_video = Video::from("https://www.example.com/video.mp4");

        assert!(!file_video.is_url());
        assert!(url_video.is_url());
    }

    #[test]
    fn test_to_path_str_with_url() {
        let db = FileStore::default();
        let video_inner = Video::from("https://www.example.com/video.mp4");

        let path_str = video_inner.to_path_str(&db);

        assert_eq!(
            path_str,
            Some("https://www.example.com/video.mp4".to_string())
        );
    }

    #[test]
    fn test_as_str_with_file() {
        let video_inner = Video::from("video.mp4");

        let inner_str = video_inner.as_str();

        assert_eq!(inner_str, "video.mp4");
    }

    #[test]
    fn test_as_str_with_url() {
        let video_inner = Video::from("https://www.example.com/video.mp4");

        let inner_str = video_inner.as_str();

        assert_eq!(inner_str, "https://www.example.com/video.mp4");
    }

    #[test]
    fn test_playlist_video_inner_to_path_str() {
        let file_store = FileStore::from_iter([FileEntry::new(
            "video.mp4".to_string(),
            PathBuf::from("/path/to/video.mp4"),
            None,
        )]);

        let file_inner = Video::from("video.mp4");
        let url_inner = Video::from("https://example.com/video.mp4");

        assert_eq!(
            file_inner.to_path_str(&file_store),
            Some("/path/to/video.mp4".to_string())
        );
        assert_eq!(
            url_inner.to_path_str(&file_store),
            Some("https://example.com/video.mp4".to_string())
        );
    }

    #[test]
    fn test_playlist_video_as_ref() {
        // Create a PlaylistVideo variant for testing
        let video = Video::from("video.mp4");

        // Use the AsRef<str> trait to get a reference to the string representation
        let video_ref: &str = video.as_ref();

        // Ensure that the reference matches the expected string
        assert_eq!(video_ref, "video.mp4");
    }

    #[test]
    fn test_playlist_video_inner_to_path_str_none() {
        // Create a dummy FileStore for testing purposes with no matching file
        let file_store = FileStore::default();

        let non_existing_file_inner = Video::from("non_existent.mp4");

        // Ensure that to_path_str returns None for a non-existing file
        assert_eq!(non_existing_file_inner.to_path_str(&file_store), None);
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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");

        playlist.push(video1.clone());
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video1));

        playlist.push(video2.clone());
        assert_eq!(playlist.len(), 2);
        assert_eq!(playlist.get(1), Some(&video2));
    }

    #[test]
    fn test_add_video_with_url_arcstr() {
        let mut playlist = Playlist::default();
        let video_url = ArcStr::from("https://www.example.com/video1.mp4");

        playlist.push(Video::from(&video_url));

        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&Video::from(&video_url)));
    }

    #[test]
    fn test_remove_from_playlist() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        assert_eq!(playlist.len(), 2);

        playlist.remove(0);
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video2));
    }

    #[test]
    fn test_remove_by_video() {
        let mut playlist = Playlist::default();
        let video_url1 = "https://www.example.com/video1.mp4";
        let video_url2 = "https://www.example.com/video2.mp4";

        playlist.push(Video::from(video_url1));
        playlist.push(Video::from(video_url2));

        let removed = playlist.remove_by_video(&Video::from(video_url1));

        assert_eq!(playlist.len(), 1);
        assert_eq!(removed, Some(Video::from(video_url1)));
        assert_eq!(playlist.get(0), Some(&Video::from(video_url2)));
    }

    #[test]
    fn test_remove_by_video_none() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");

        playlist.push(video1.clone());
        assert_eq!(playlist.len(), 1);

        // Attempt to remove Video 2, which is not in the playlist.
        let removed_video = playlist.remove_by_video(&video2);

        // Verify that the playlist remains unchanged.
        assert_eq!(playlist.len(), 1);
        assert_eq!(playlist.get(0), Some(&video1));

        // Verify that the remove_by_video method returns None.
        assert_eq!(removed_video, None);
    }

    #[test]
    fn test_get_range() {
        let mut playlist = Playlist::default();

        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

        // Get a range of videos from index 1 to 3 (inclusive).
        let range_result: Vec<_> = playlist.get_range(1, 3).collect();

        // Verify that the returned range contains the expected videos.
        assert_eq!(range_result.len(), 3);
        assert_eq!(range_result.first(), Some(&&video2));
        assert_eq!(range_result.get(1), Some(&&video3));
        assert_eq!(range_result.get(2), Some(&&video4));
    }

    #[test]
    fn test_move_video_within_playlist() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());

        // Move video2 to the beginning
        playlist.move_video(&video2, 0);
        assert_eq!(playlist.get(0), Some(&video2));
        assert_eq!(playlist.get(1), Some(&video1));
        assert_eq!(playlist.get(2), Some(&video3));
    }

    #[test]
    fn test_move_video_within_playlist_back() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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
        let video1 = Video::from("Video 1");
        // Moving in an empty playlist should not panic.
        playlist.move_video(&video1, 0);
    }

    #[test]
    fn test_iteration() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");

        playlist.push(video1.clone());
        playlist.push(video2.clone());

        let cloned_playlist = playlist.clone();

        // Modify the original playlist.
        playlist.remove(0);

        // Verify that the cloned playlist remains unchanged.
        assert_eq!(cloned_playlist.len(), 2);
        assert_eq!(cloned_playlist.get(0), Some(&video1));
        assert_eq!(cloned_playlist.get(1), Some(&video2));
    }

    #[test]
    fn test_move_video_new_index_greater_than_old_index() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());

        // Move Video 2 to a new index greater than its old index.
        playlist.move_video(&video2, 2);

        // Verify that Video 2 and 3 are in the correct position.
        assert_eq!(playlist.get(1), Some(&video3));
        assert_eq!(playlist.get(2), Some(&video2));
    }

    #[test]
    fn test_insert_into_playlist() {
        let mut playlist = Playlist::default();
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");

        playlist.push(video1.clone());
        playlist.push(video3.clone());

        // Insert Video 2 at index 1.
        playlist.insert(1, video2.clone());

        // Verify the resulting playlist.
        assert_eq!(playlist.len(), 3);
        assert_eq!(playlist.get(0), Some(&video1));
        assert_eq!(playlist.get(1), Some(&video2));
        assert_eq!(playlist.get(2), Some(&video3));
    }

    #[test]
    fn test_move_first_element_to_right_and_back() {
        let mut playlist = Playlist::default();

        // Initialize the playlist with 5 elements.
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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

        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");

        playlist.push(video1.clone());
        playlist.push(video2.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("Video 2");
        let video3 = Video::from("Video 3");
        let video4 = Video::from("Video 4");
        let video5 = Video::from("Video 5");

        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video3.clone());
        playlist.push(video4.clone());
        playlist.push(video5.clone());

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
        let video1 = Video::from("Video 1");
        let video2 = Video::from("http://video_2");

        // Append Video 1 twice to create a duplicate.
        playlist.push(video1.clone());
        playlist.push(video2.clone());
        playlist.push(video1.clone());

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
