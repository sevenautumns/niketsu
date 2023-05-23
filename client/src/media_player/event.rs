use std::process::exit;
use std::time::Duration;

use anyhow::Result;
use enum_dispatch::enum_dispatch;
use log::debug;

use crate::client::message::ClientMessage;
use crate::client::server::ServerMessage;
use crate::client::ClientInner;
use crate::user::ThisUser;
use crate::video::PlayingFile;

#[enum_dispatch(ClientMessage)]
#[derive(Debug, Clone)]
pub enum MediaPlayerEvent {
    Paused,
    Started,
    PositionChanged,
    SpeedChanged,
    PlaybackEnded,
    Exit,
}

#[derive(Debug, Clone)]
pub struct Paused;

impl Paused {
    fn set_not_ready(client: &mut ClientInner) -> Option<ServerMessage> {
        let mut state = None;
        client.user.rcu(|u| {
            let mut user = ThisUser::clone(u);
            state = user.set_ready(false);
            user
        });
        state
    }

    fn set_paused(client: &mut ClientInner) {
        client.player.playing_file_mut(|file| {
            if let Some(file) = file {
                file.paused = true;
            }
        })
    }
}

impl ClientMessage for Paused {
    fn handle(self, client: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: pause");
        Self::set_paused(client);
        let state = Self::set_not_ready(client);

        if let Some(state) = state {
            client.ws.load().send(state)?;
        }
        client.ws.load().send(ServerMessage::Pause {
            username: client.user.load().name(),
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Started;

impl Started {
    fn set_ready(client: &mut ClientInner) -> Option<ServerMessage> {
        let mut state = None;
        client.user.rcu(|u| {
            let mut user = ThisUser::clone(u);
            state = user.set_ready(true);
            user
        });
        state
    }

    fn set_playing(client: &mut ClientInner) {
        client.player.playing_file_mut(|file| {
            if let Some(file) = file {
                file.paused = false;
            }
        })
    }
}

impl ClientMessage for Started {
    fn handle(self, client: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: start");
        Self::set_playing(client);
        let state = Self::set_ready(client);

        if let Some(state) = state {
            client.ws.load().send(state)?;
        }
        client.ws.load().send(ServerMessage::Start {
            username: client.user.load().name(),
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PositionChanged(pub Duration);

impl ClientMessage for PositionChanged {
    fn handle(self, client: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: seek {:?}", self.0);
        let Some(playing) = client.player.playing_file() else  {
            return Ok(());
        };
        client.ws.load().send(ServerMessage::Seek {
            filename: playing.video.as_str().to_string(),
            position: self.0,
            username: client.user.load().name(),
            paused: playing.paused,
            desync: false,
            speed: playing.speed,
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SpeedChanged(pub f64);

impl ClientMessage for SpeedChanged {
    fn handle(self, client: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: playback speed");
        client.player.playing_file_mut(|file| {
            if let Some(file) = file.as_mut() {
                file.speed = self.0;
            };
        });

        client.ws.load().send(ServerMessage::PlaybackSpeed {
            username: client.user.load().name(),
            speed: self.0,
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackEnded;

impl ClientMessage for PlaybackEnded {
    fn handle(self, client: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: play next");
        let Some(file) = client.player.playing_file() else {
            return Ok(());
        };
        if let Some(next) = client.playlist_widget.load().next_video(&file.video) {
            client.ws.load().send(ServerMessage::Select {
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
            client.ws.load().send(ServerMessage::Select {
                filename: None,
                username: client.user.load().name(),
            })?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Exit;

impl ClientMessage for Exit {
    fn handle(self, _: &mut ClientInner) -> anyhow::Result<()> {
        debug!("Mpv process: exit");
        exit(0)
    }
}
