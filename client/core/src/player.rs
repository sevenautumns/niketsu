use std::process::exit;
use std::time::Duration;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use ordered_float::OrderedFloat;
use url::Url;

use super::communicator::{
    NiketsuPause, NiketsuPlaybackSpeed, NiketsuSeek, NiketsuStart, OutgoingMessage,
};
use super::file_database::FileEntry;
use super::playlist::PlaylistVideo;
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
    fn load_video(&mut self, load: LoadVideo);
    fn unload_video(&mut self);
    fn playing_video(&self) -> Option<PlayerVideo>;
    async fn event(&mut self) -> MediaPlayerEvent;
}

pub trait MediaPlayerTraitExt {
    fn load_playlist_video(&mut self, video: &PlaylistVideo, db: &FileStore) -> bool;
}

impl<T: ?Sized + MediaPlayerTrait> MediaPlayerTraitExt for T {
    fn load_playlist_video(&mut self, video: &PlaylistVideo, db: &FileStore) -> bool {
        let Some(player_video) = video.to_player_video(db) else {
            return false;
        };
        let load_video = player_video.into_load_video(self);
        self.load_video(load_video);
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerVideo {
    Url(Url),
    File(FileEntry),
}

impl PlayerVideo {
    pub fn path_str(&self) -> Option<&str> {
        match self {
            PlayerVideo::Url(url) => Some(url.as_str()),
            PlayerVideo::File(entry) => entry.path().to_str(),
        }
    }

    pub fn name_str(&self) -> &str {
        match self {
            PlayerVideo::Url(url) => url.as_str(),
            PlayerVideo::File(entry) => entry.file_name(),
        }
    }

    pub fn into_load_video<T: ?Sized + MediaPlayerTrait>(self, player: &T) -> LoadVideo {
        LoadVideo {
            video: self,
            pos: Duration::ZERO,
            speed: player.get_speed(),
            paused: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadVideo {
    pub video: PlayerVideo,
    pub pos: Duration,
    pub speed: f64,
    pub paused: bool,
}

impl PartialEq for LoadVideo {
    fn eq(&self, other: &Self) -> bool {
        let speed_self = OrderedFloat(self.speed);
        let speed_other = OrderedFloat(self.speed);
        speed_self.eq(&speed_other)
            && self.video.eq(&other.video)
            && self.pos.eq(&other.pos)
            && self.paused.eq(&other.paused)
    }
}

impl Eq for LoadVideo {}

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
        model.user.not_ready();
        model
            .communicator
            .send(OutgoingMessage::from(model.user.clone()));
        model.communicator.send(
            NiketsuPause {
                actor: model.user.name.clone(),
            }
            .into(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PlayerStart;

impl EventHandler for PlayerStart {
    fn handle(self, model: &mut CoreModel) {
        model.user.ready();
        model
            .communicator
            .send(OutgoingMessage::from(model.user.clone()));
        model.communicator.send(
            NiketsuStart {
                actor: model.user.name.clone(),
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
        let file = playing.name_str().to_string();
        let Some(paused) = model.player.is_paused() else {
            return;
        };
        let speed = model.player.get_speed();
        let actor = model.user.name.clone();
        let position = self.pos;

        model.communicator.send(
            NiketsuSeek {
                actor,
                file,
                paused,
                speed,
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
        let actor = model.user.name.clone();
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
            if !model
                .player
                .load_playlist_video(&next, model.database.all_files())
            {
                model.player.unload_video();
            }
            model.ui.video_change(Some(next));
        } else {
            model.player.unload_video();
            model.ui.video_change(None);
        }
        let actor = model.user.name.clone();
        model
            .communicator
            .send(NiketsuSelect { actor, filename }.into());
    }
}

#[derive(Debug, Clone)]
pub struct PlayerExit;

impl EventHandler for PlayerExit {
    fn handle(self, _: &mut CoreModel) {
        exit(0)
    }
}
