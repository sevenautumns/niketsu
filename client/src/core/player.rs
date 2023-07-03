use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use url::Url;

#[async_trait]
pub trait MediaPlayer {
    fn start(&mut self);
    fn pause(&mut self);
    fn is_paused(&self) -> Result<bool>;
    fn set_speed(&mut self, speed: f64);
    fn get_speed(&self) -> f64;
    fn set_position(&mut self, pos: Duration);
    fn get_position(&mut self) -> Duration;
    fn load_video(&mut self, video: Video);
    fn unload_video(&mut self);
    fn playing_video(&self) -> Option<Video>;
    async fn event(&mut self) -> MediaPlayerEvent;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Video {
    Url(Url),
    File(PathBuf),
}

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

impl From<PlayerPause> for MediaPlayerEvent {
    fn from(value: PlayerPause) -> Self {
        Self::Pause(value)
    }
}

#[derive(Debug, Clone)]
pub struct PlayerStart;

impl From<PlayerStart> for MediaPlayerEvent {
    fn from(value: PlayerStart) -> Self {
        Self::Start(value)
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

impl From<PlayerPositionChange> for MediaPlayerEvent {
    fn from(value: PlayerPositionChange) -> Self {
        Self::PositionChange(value)
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

impl From<PlayerSpeedChange> for MediaPlayerEvent {
    fn from(value: PlayerSpeedChange) -> Self {
        Self::SpeedChange(value)
    }
}

#[derive(Debug, Clone)]
pub struct PlayerFileEnd;

impl From<PlayerFileEnd> for MediaPlayerEvent {
    fn from(value: PlayerFileEnd) -> Self {
        Self::FileEnd(value)
    }
}

#[derive(Debug, Clone)]
pub struct PlayerExit;

impl From<PlayerExit> for MediaPlayerEvent {
    fn from(value: PlayerExit) -> Self {
        Self::Exit(value)
    }
}
