use std::time::Duration;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tracing::{trace, warn};

use super::communicator::Intent;
use super::playlist::Video;
use super::{CoreModel, EventHandler};
use crate::FilePathSearch;
use crate::file_database::FileStore;

pub mod wrapper;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MediaPlayerTrait: std::fmt::Debug + Send {
    fn start(&mut self);
    fn pause(&mut self);
    fn is_paused(&self) -> Option<bool>;
    fn set_speed(&mut self, speed: f64);
    fn set_position(&mut self, pos: Duration);
    fn get_position(&mut self) -> Option<Duration>;
    // TODO separate FileStore from MediaPlayer
    // for this we need to move the file_loaded out of the player
    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore);
    fn unload_video(&mut self);
    fn maybe_reload_video(&mut self, f: &dyn FilePathSearch);
    fn reload_video(&mut self, f: &dyn FilePathSearch, filename: &str);
    fn playing_video(&self) -> Option<Video>;
    fn video_loaded(&self) -> bool;
    async fn event(&mut self) -> MediaPlayerEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum MediaPlayerEvent {
    Pause(PlayerPause),
    Start(PlayerStart),
    CachePause(PlayerCachePause),
    PositionChange(PlayerPositionChange),
    SpeedChange(PlayerSpeedChange),
    FileEnd(PlayerFileEnd),
    Exit(PlayerExit),
}

#[derive(Debug, Clone)]
pub struct PlayerPause;

impl EventHandler for PlayerPause {
    fn handle(self, model: &mut CoreModel) {
        trace!("player pause");
        model.request(Intent::Pause);
        model.ready = false;
        model.ui.user_update(crate::ui::UserChange {
            name: model.config.username.clone(),
            ready: false,
        });
        model.publish_status();
    }
}

#[derive(Debug, Clone)]
pub struct PlayerStart;

impl EventHandler for PlayerStart {
    fn handle(self, model: &mut CoreModel) {
        trace!("player start");
        model.request(Intent::Start);
        model.ready = true;
        model.ui.user_update(crate::ui::UserChange {
            name: model.config.username.clone(),
            ready: true,
        });
        model.publish_status();
    }
}

/// The player waits for its cache to fill, and the room with it. The user
/// stays as ready as before.
#[derive(Debug, Clone)]
pub struct PlayerCachePause;

impl EventHandler for PlayerCachePause {
    fn handle(self, model: &mut CoreModel) {
        trace!("player cache pause");
        model.request(Intent::Pause);
    }
}

#[derive(Debug, Clone)]
pub struct PlayerPositionChange {
    pos: Duration,
}

impl PlayerPositionChange {
    pub fn new(pos: Duration) -> Self {
        Self { pos }
    }
}

impl EventHandler for PlayerPositionChange {
    fn handle(self, model: &mut CoreModel) {
        trace!("player position change");
        let Some(video) = model.player.playing_video() else {
            return;
        };
        let position = self.pos;
        model.request(Intent::Seek { video, position });
    }
}

#[derive(Debug, Clone)]
pub struct PlayerSpeedChange {
    speed: f64,
}

impl PlayerSpeedChange {
    pub fn new(speed: f64) -> Self {
        Self { speed }
    }
}

impl EventHandler for PlayerSpeedChange {
    fn handle(self, model: &mut CoreModel) {
        trace!("player speed change");
        model.request(Intent::Speed(self.speed));
    }
}

#[derive(Debug, Clone)]
pub struct PlayerFileEnd(pub Video);

impl EventHandler for PlayerFileEnd {
    fn handle(self, model: &mut CoreModel) {
        trace!("player file end");
        model.video_server.stop_server();

        let current_video = model.playlist.get_current_video();
        if current_video.as_ref().is_none_or(|c| c.ne(&self.0)) {
            warn!(
                playlist_video = ?current_video,
                mpv_video = ?self.0,
                "current playlist video and mpv video do not match",
            );
            return;
        }

        let video = model.playlist.next_video();
        let position = Duration::ZERO;
        model.request(Intent::Select { video, position });
    }
}

#[derive(Debug, Clone)]
pub struct PlayerExit;

impl EventHandler for PlayerExit {
    fn handle(self, model: &mut CoreModel) {
        trace!("player exit");
        model.running = false;
        model.ui.abort();
    }
}
