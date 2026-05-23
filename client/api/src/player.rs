use std::future;
use std::time::Duration;

use async_trait::async_trait;
use niketsu_core::file_database::{FilePathSearch, FileStore};
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait};
use niketsu_core::playlist::Video;

#[derive(Debug, Default)]
pub struct NoopPlayer;

#[async_trait]
impl MediaPlayerTrait for NoopPlayer {
    fn start(&mut self) {}
    fn pause(&mut self) {}
    fn is_paused(&self) -> Option<bool> { None }
    fn set_speed(&mut self, _speed: f64) {}
    fn get_speed(&self) -> f64 { 1.0 }
    fn set_position(&mut self, _pos: Duration) {}
    fn get_position(&mut self) -> Option<Duration> { None }
    fn cache_available(&mut self) -> bool { false }
    fn load_video(&mut self, _load: Video, _pos: Duration, _db: &FileStore) {}
    fn unload_video(&mut self) {}
    fn maybe_reload_video(&mut self, _f: &dyn FilePathSearch) {}
    fn reload_video(&mut self, _f: &dyn FilePathSearch, _filename: &str) {}
    fn playing_video(&self) -> Option<Video> { None }
    fn video_loaded(&self) -> bool { false }

    async fn event(&mut self) -> MediaPlayerEvent {
        future::pending().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_player_returns_no_state() {
        let mut player = NoopPlayer;
        assert!(player.playing_video().is_none());
        assert!(player.get_position().is_none());
        assert!(player.is_paused().is_none());
        assert!(!player.video_loaded());
        assert!(!player.cache_available());
    }
}
