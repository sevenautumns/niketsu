use std::process::exit;
use std::time::Duration;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;

use super::communicator::{
    NiketsuPause, NiketsuPlaybackSpeed, NiketsuSeek, NiketsuStart, OutgoingMessage,
};
use super::playlist::Video;
use super::{CoreModel, EventHandler};
use crate::communicator::NiketsuSelect;
use crate::file_database::FileStore;

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
    // TODO separate FileStore from MediaPlayer
    // for this we need to move the file_loaded out of the player
    fn load_video(&mut self, load: Video, pos: Duration, db: &FileStore);
    fn unload_video(&mut self);
    fn maybe_reload_video(&mut self, db: &FileStore);
    fn playing_video(&self) -> Option<Video>;
    fn video_loaded(&self) -> bool;
    async fn event(&mut self) -> MediaPlayerEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum MediaPlayerEvent {
    Pause(PlayerPause),
    Start(PlayerStart),
    PositionChange(PlayerPositionChange),
    SpeedChange(PlayerSpeedChange),
    FileEnd(PlayerFileEnd),
    Exit(PlayerExit),
}

#[derive(Debug, Clone)]
pub struct PlayerPause;

impl EventHandler for PlayerPause {
    fn handle(self, model: &mut CoreModel) {
        model.ready = false;
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.communicator.send(
            NiketsuPause {
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
        model.ready = true;
        model
            .communicator
            .send(OutgoingMessage::from(model.config.status(model.ready)));
        model.communicator.send(
            NiketsuStart {
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
        let Some(playing) = model.player.playing_video() else {
            return;
        };
        let file = playing.as_str().to_string();
        let actor = model.config.username.clone();
        let position = self.pos;

        model.communicator.send(
            NiketsuSeek {
                actor,
                file,
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
        let speed = self.speed;
        let actor = model.config.username.clone();
        model
            .communicator
            .send(NiketsuPlaybackSpeed { actor, speed }.into());
    }
}

#[derive(Debug, Clone)]
pub struct PlayerFileEnd;

impl EventHandler for PlayerFileEnd {
    fn handle(self, model: &mut CoreModel) {
        // TODO refactor
        let mut filename = None;
        if let Some(next) = model.playlist.advance_to_next() {
            filename = Some(next.as_str().to_string());
            model
                .player
                .load_video(next.clone(), Duration::ZERO, model.database.all_files());
            model.ui.video_change(Some(next));
        } else {
            model.player.unload_video();
            model.ui.video_change(None);
        }
        let actor = model.config.username.clone();
        let position = model.player.get_position().unwrap_or_default();
        model.communicator.send(
            NiketsuSelect {
                actor,
                filename,
                position,
            }
            .into(),
        );
    }
}

#[derive(Debug, Clone)]
pub struct PlayerExit;

impl EventHandler for PlayerExit {
    fn handle(self, _: &mut CoreModel) {
        exit(0)
    }
}
