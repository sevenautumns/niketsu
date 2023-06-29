use actix::{Handler, Message};
use anyhow::Result;

use super::actor::Player;
use super::MediaPlayer;
use crate::file_system::actor::FileDatabaseModel;
use crate::video::PlayingFile;

#[derive(Message)]
#[rtype(result = "anyhow::Result<()>")]
pub struct PausePlayer;

impl<M: MediaPlayer, F: FileDatabaseModel> Handler<PausePlayer> for Player<M, F> {
    type Result = Result<()>;

    fn handle(&mut self, _: PausePlayer, _: &mut Self::Context) -> Self::Result {
        self.pause()
    }
}

#[derive(Message)]
#[rtype(result = "anyhow::Result<()>")]
pub struct StartPlayer;

impl<M: MediaPlayer, F: FileDatabaseModel> Handler<StartPlayer> for Player<M, F> {
    type Result = Result<()>;

    fn handle(&mut self, _: StartPlayer, _: &mut Self::Context) -> Self::Result {
        self.start()
    }
}

#[derive(Message)]
#[rtype(result = "anyhow::Result<()>")]
pub struct SpeedPlayer {
    speed: f64,
}

impl<M: MediaPlayer, F: FileDatabaseModel> Handler<SpeedPlayer> for Player<M, F> {
    type Result = Result<()>;

    fn handle(&mut self, msg: SpeedPlayer, _: &mut Self::Context) -> Self::Result {
        self.set_speed(msg.speed)
    }
}

#[derive(Message)]
#[rtype(result = "anyhow::Result<()>")]
pub struct LoadFilePlayer {
    play: PlayingFile,
}

impl<M: MediaPlayer, F: FileDatabaseModel> Handler<LoadFilePlayer> for Player<M, F> {
    type Result = Result<()>;

    fn handle(&mut self, msg: LoadFilePlayer, _: &mut Self::Context) -> Self::Result {
        let Some(file) = &self.file else {
            return self.open(msg.play);
        };
        if file.video.as_str().eq(msg.play.video.as_str()) {
            return self.seek(msg.play);
        }
        self.open(msg.play)
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct UnloadFilePlayer;

impl<M: MediaPlayer, F: FileDatabaseModel> Handler<UnloadFilePlayer> for Player<M, F> {
    type Result = ();

    fn handle(&mut self, _: UnloadFilePlayer, _: &mut Self::Context) {
        self.file = None;
        self.file_loaded = false;
    }
}

impl<M: MediaPlayer, F: FileDatabaseModel> Player<M, F> {
    fn pause(&mut self) -> Result<()> {
        let Some(file) = &mut self.file else {
            return Ok(());
        };
        if file.paused {
            return Ok(());
        }
        file.paused = true;
        self.player.pause()
    }

    fn start(&mut self) -> Result<()> {
        let Some(file) = &mut self.file else {
            return Ok(());
        };
        if !file.paused {
            return Ok(());
        }
        file.paused = false;
        self.player.start()
    }

    fn set_speed(&mut self, speed: f64) -> Result<()> {
        let Some(file) = &mut self.file else {
            return Ok(());
        };
        if file.speed == speed {
            return Ok(());
        }
        file.speed = speed;
        self.player.set_speed(speed)
    }

    fn seek(&mut self, play: PlayingFile) -> Result<()> {
        if self.file.is_none() {
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
        self.file = Some(play.clone());
        if let Some(path) = play.video.to_path_str_new(&self.db) {
            self.file_loaded = true;
            self.player.open(path, play.paused, play.pos)
        } else {
            self.file_loaded = false;
            Ok(())
        }
    }
}
