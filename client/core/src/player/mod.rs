use std::time::Duration;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tracing::{trace, warn};

use super::communicator::{
    OutgoingMessage, PauseMsg, PlaybackSpeedMsg, SeekMsg, SelectMsg, StartMsg,
};
use super::playlist::Video;
use super::{CoreModel, EventHandler};
use crate::FilePathSearch;
use crate::file_database::FileStore;
use crate::playlist::file::PlaylistBrowser;

pub mod wrapper;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MediaPlayerTrait: std::fmt::Debug + Send {
    fn start(&mut self);
    fn pause(&mut self);
    fn is_paused(&self) -> Option<bool>;
    fn set_speed(&mut self, speed: f64);
    fn get_speed(&self) -> f64;
    fn set_position(&mut self, pos: Duration);
    fn get_position(&mut self) -> Option<Duration>;
    fn cache_available(&mut self) -> bool;
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
        model.ready = false;
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.communicator.send(
            PauseMsg {
                actor: model.config.username.clone(),
            }
            .into(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PlayerStart;

impl EventHandler for PlayerStart {
    fn handle(self, model: &mut CoreModel) {
        trace!("player start");
        model.ready = true;
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.communicator.send(
            StartMsg {
                actor: model.config.username.clone(),
            }
            .into(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PlayerCachePause;

impl EventHandler for PlayerCachePause {
    fn handle(self, model: &mut CoreModel) {
        trace!("player cache pause");
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.communicator.send(
            PauseMsg {
                actor: model.config.username.clone(),
            }
            .into(),
        )
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
        let actor = model.config.username.clone();
        let position = self.pos;

        model.communicator.send(
            SeekMsg {
                actor,
                video,
                position,
            }
            .into(),
        );
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
        let speed = self.speed;
        let actor = model.config.username.clone();
        model
            .communicator
            .send(PlaybackSpeedMsg { actor, speed }.into());
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

        // TODO refactor
        let mut video = None;
        if let Some(next) = model.playlist.advance_to_next() {
            video = Some(next.clone());
            model
                .player
                .load_video(next.clone(), Duration::ZERO, model.database.all_files());
            model.ui.video_change(Some(next));
        } else {
            model.player.unload_video();
            model.ui.video_change(None);
        }
        PlaylistBrowser::save(&model.config.room, &model.playlist);
        let actor = model.config.username.clone();
        let position = model.player.get_position().unwrap_or_default();
        model.communicator.send(
            SelectMsg {
                actor,
                video,
                position,
            }
            .into(),
        );
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
