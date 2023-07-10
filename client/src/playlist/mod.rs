use crate::core::playlist::PlaylistTrait;

#[derive(Debug)]
pub struct Playlist {
    playing: usize,
    list: Vec<String>,
}

impl Playlist {
    fn find(&self, video: &str) -> Option<usize> {
        self.list
            .iter()
            .enumerate()
            .find(|(_, v)| v.as_str().eq(video))
            .map(|(i, _)| i)
    }
}

impl PlaylistTrait for Playlist {
    fn replace(&mut self, playlist: Vec<String>) {
        self.list = playlist;
    }

    fn append(&mut self, video: String) {
        self.list.push(video);
    }

    fn get_current_video(&self) -> Option<String> {
        self.list.get(self.playing).cloned()
    }

    fn advance(&mut self) -> Option<String> {
        self.playing += 1;
        self.get_current_video()
    }

    fn play(&mut self, video: &str) {
        if let Some(index) = self.find(video) {
            self.playing = index;
        }
    }

    fn get(&self) -> Vec<String> {
        self.list.clone()
    }

    fn delete_video(&mut self, video: &str) {
        let Some(index) = self.find(video) else {
            return;
        };
        let was_playing = self.get_current_video().is_some();
        self.list.remove(index);
        let is_playing = self.get_current_video().is_some();
        if was_playing && !is_playing {
            self.playing = self.playing.saturating_sub(1);
        }
    }

    fn move_video(&mut self, video: &str, index: usize) {
        let mut new_index = index;
        if let Some(old_index) = self.find(video) {
            if new_index > old_index {
                new_index -= 1;
            }
            let video = self.list.remove(old_index);
            new_index = new_index.min(self.list.len());
            self.list.insert(new_index, video);
        } else {
            new_index = new_index.min(self.list.len());
            self.list.insert(new_index, video.to_string());
        }
    }
}
