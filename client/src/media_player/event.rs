use std::process::exit;
use std::time::Duration;

use anyhow::Result;
use enum_dispatch::enum_dispatch;
use log::debug;

use super::{MediaPlayer, MediaPlayerWrapper};
use crate::client::message::CoreMessageTrait;
use crate::client::server::{
    NiketsuMessage, NiketsuPause, NiketsuPlaybackSpeed, NiketsuSeek, NiketsuSelect, NiketsuStart,
};
use crate::client::CoreRunner;
use crate::user::ThisUser;
use crate::video::PlayingFile;

#[enum_dispatch]
pub trait MediaPlayerEventTrait {
    fn handle<M: MediaPlayer>(self, player: &mut MediaPlayerWrapper<M>) -> Result<()>;
}

#[enum_dispatch(MediaPlayerEventTrait, CoreMessageTrait)]
#[derive(Debug, Clone)]
pub enum MediaPlayerEvent {
    PlayerPaused,
    PlayerStarted,
    PlayerPositionChanged,
    PlayerSpeedChanged,
    PlayerPlaybackEnded,
    PlayerExit,
}

#[derive(Debug, Clone)]
pub struct PlayerPaused;

impl PlayerPaused {
    fn set_not_ready(client: &mut CoreRunner) -> Option<NiketsuMessage> {
        let mut state = None;
        client.user.rcu(|u| {
            let mut user = ThisUser::clone(u);
            state = user.set_ready(false);
            user
        });
        state
    }
}

impl CoreMessageTrait for PlayerPaused {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        let state = Self::set_not_ready(client);

        if let Some(state) = state {
            client.ws.sender().send(state)?;
        }
        client.ws.sender().send(NiketsuPause {
            username: client.user.load().name(),
        })
    }
}

impl MediaPlayerEventTrait for PlayerPaused {
    fn handle<M: MediaPlayer>(self, player: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: pause");
        if let Some(file) = &mut player.status.file {
            file.paused = true;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerStarted;

impl PlayerStarted {
    fn set_ready(client: &mut CoreRunner) -> Option<NiketsuMessage> {
        let mut state = None;
        client.user.rcu(|u| {
            let mut user = ThisUser::clone(u);
            state = user.set_ready(true);
            user
        });
        state
    }
}

impl CoreMessageTrait for PlayerStarted {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        let state = Self::set_ready(client);

        if let Some(state) = state {
            client.ws.sender().send(state)?;
        }
        client.ws.sender().send(NiketsuStart {
            username: client.user.load().name(),
        })
    }
}

impl MediaPlayerEventTrait for PlayerStarted {
    fn handle<M: MediaPlayer>(self, player: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: start");
        if let Some(file) = &mut player.status.file {
            file.paused = false;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerPositionChanged(pub Duration);

impl CoreMessageTrait for PlayerPositionChanged {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        let Some(playing) = client.player.playing_file() else {
            return Ok(());
        };
        client.ws.sender().send(NiketsuSeek {
            filename: playing.video.as_str().to_string(),
            position: self.0,
            username: client.user.load().name(),
            paused: playing.paused,
            desync: false,
            speed: playing.speed,
        })
    }
}

impl MediaPlayerEventTrait for PlayerPositionChanged {
    fn handle<M: MediaPlayer>(self, _: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: seek {:?}", self.0);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerSpeedChanged(pub f64);

impl CoreMessageTrait for PlayerSpeedChanged {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        client.ws.sender().send(NiketsuPlaybackSpeed {
            username: client.user.load().name(),
            speed: self.0,
        })
    }
}

impl MediaPlayerEventTrait for PlayerSpeedChanged {
    fn handle<M: MediaPlayer>(self, player: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: playback speed");
        if let Some(file) = &mut player.status.file {
            file.speed = self.0;
        };
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerPlaybackEnded;

impl CoreMessageTrait for PlayerPlaybackEnded {
    fn handle(self, client: &mut CoreRunner) -> Result<()> {
        let Some(file) = client.player.playing_file() else {
            return Ok(());
        };
        if let Some(next) = client.playlist_widget.load().next_video(&file.video) {
            client.ws.sender().send(NiketsuSelect {
                filename: next.as_str().to_string().into(),
                username: client.user.load().name(),
            })?;
            let next = PlayingFile {
                video: next,
                paused: true,
                speed: client.player.get_speed()?,
                pos: Duration::ZERO,
            };
            client.player.load(next)
        } else {
            client.ws.sender().send(NiketsuSelect {
                filename: None,
                username: client.user.load().name(),
            })?;
            Ok(())
        }
    }
}

impl MediaPlayerEventTrait for PlayerPlaybackEnded {
    fn handle<M: MediaPlayer>(self, _: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: play next");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerExit;

impl CoreMessageTrait for PlayerExit {
    fn handle(self, _: &mut CoreRunner) -> Result<()> {
        exit(0)
    }
}

impl MediaPlayerEventTrait for PlayerExit {
    fn handle<M: MediaPlayer>(self, _: &mut MediaPlayerWrapper<M>) -> anyhow::Result<()> {
        debug!("Mpv process: exit");
        Ok(())
    }
}
