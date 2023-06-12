use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use self::event::MediaPlayerEvent;
use self::mpv::MpvHandle;
use crate::file_system::FileDatabaseProxy;
use crate::video::PlayingFile;

pub mod event;
pub mod mpv;

#[async_trait]
pub trait MediaPlayer: Sized {
    fn new() -> Result<Self>;
    fn pause(&mut self) -> Result<()>;
    fn is_paused(&self) -> Result<bool>;
    fn is_seeking(&self) -> Result<bool>;
    fn start(&mut self) -> Result<()>;
    fn set_speed(&mut self, speed: f64) -> Result<()>;
    fn get_speed(&self) -> Result<f64>;
    fn set_position(&mut self, pos: Duration) -> Result<()>;
    fn get_position(&self) -> Result<Duration>;
    fn open(&self, path: String, paused: bool, pos: Duration) -> Result<()>;
    #[must_use]
    async fn receive_event(&mut self) -> Result<MediaPlayerEvent>;
}

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerStatus {
    file: Option<PlayingFile>,
    file_loaded: bool,
}

#[derive(Debug)]
pub struct MediaPlayerWrapper<M: MediaPlayer> {
    player: M,
    db: Arc<FileDatabaseProxy>,
    status: MediaPlayerStatus,
}

impl<M: MediaPlayer> MediaPlayerWrapper<M> {
    pub fn new(db: Arc<FileDatabaseProxy>) -> Result<Self> {
        let player = M::new()?;

        Ok(Self {
            player,
            status: Default::default(),
            db,
        })
    }

    pub fn pause(&mut self) -> Result<()> {
        let Some(file) = self.status.file.as_mut() else {
            return Ok(());
        };
        if file.paused {
            return Ok(());
        }
        file.paused = true;
        self.player.pause()
    }

    pub fn is_paused(&self) -> Result<bool> {
        if let Some(file) = &self.status.file {
            return Ok(file.paused);
        }
        self.player.is_paused()
    }

    pub fn start(&mut self) -> Result<()> {
        let Some(file) = self.status.file.as_mut() else {
            return Ok(());
        };
        if !file.paused {
            return Ok(());
        }
        file.paused = false;
        self.player.start()
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<()> {
        let Some(file) = self.status.file.as_mut() else {
            return Ok(());
        };
        if file.speed == speed {
            return Ok(());
        }
        file.speed = speed;
        self.player.set_speed(speed)
    }

    pub fn get_speed(&self) -> Result<f64> {
        if let Some(file) = &self.status.file {
            return Ok(file.speed);
        }
        self.player.get_speed()
    }

    pub fn get_position(&self) -> Result<Duration> {
        self.player.get_position()
    }

    pub fn load(&mut self, play: PlayingFile) -> Result<()> {
        let Some(file) = &self.status.file else {
            return self.open(play);
        };
        if file.video.as_str().eq(play.video.as_str()) {
            return self.seek(play);
        }
        self.open(play)
    }

    pub fn unload(&mut self) {
        self.status.file = None;
        self.status.file_loaded = false;
    }

    fn seek(&mut self, play: PlayingFile) -> Result<()> {
        if self.status.file.is_none() {
            return Ok(());
        }
        if play.paused {
            self.pause()?;
        } else {
            self.start()?;
        }
        self.set_speed(play.speed)?;
        self.player.set_position(play.pos)
    }

    fn open(&mut self, play: PlayingFile) -> Result<()> {
        self.status.file = Some(play.clone());
        if let Some(path) = play.video.to_path_str(&self.db) {
            self.status.file_loaded = true;
            self.player.open(path, play.paused, play.pos)
        } else {
            self.status.file_loaded = false;
            Ok(())
        }
    }

    pub fn retry_load(&mut self) -> Result<()> {
        if self.status.file_loaded {
            return Ok(());
        };
        let Some(file) = self.status.file.clone() else {
            return Ok(());
        };
        let Some(path) = file.video.to_path_str(&self.db) else {
            return Ok(());
        };
        self.status.file_loaded = true;
        self.player.open(path, file.paused, file.pos)
    }

    pub fn playing_file(&self) -> Option<PlayingFile> {
        if let Some(file) = self.status.file.clone() {
            return Some(file);
        }
        None
    }

    pub fn is_seeking(&self) -> Result<bool> {
        self.player.is_seeking()
    }

    pub async fn recv(&mut self) -> Result<MediaPlayerEvent> {
        self.player.receive_event().await
    }
}
