pub trait PlaylistTrait {
    fn replace(&mut self, playlist: Vec<String>);
    fn append(&mut self, video: String);
    fn get_current_video(&self) -> Option<String>;
    fn advance(&mut self) -> Option<String>;
    fn play(&mut self, video: &str);
    fn get(&self) -> Vec<String>;
    fn delete_video(&mut self, video: &str);
    fn move_video(&mut self, video: &str, index: usize);
}

pub trait PlaylistTraitExt {
    fn delete_video_get_new_current(&mut self, video: &str) -> Option<String>;
}

impl<P: PlaylistTrait> PlaylistTraitExt for P {
    fn delete_video_get_new_current(&mut self, video: &str) -> Option<String> {
        let old = self.get_current_video();
        self.delete_video(video);
        let new = self.get_current_video();
        if old != new {
            return new;
        }
        None
    }
}
