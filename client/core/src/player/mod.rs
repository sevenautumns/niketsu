use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use multiaddr::PeerId;
use tokio::time::Instant;
use tracing::trace;

use super::chunk::SharedFile;
use super::communicator::Intent;
use super::ui::PlayerMessage;
use super::{CoreModel, EventHandler};

pub mod offset;

/// The player follows the room: the core tells it what to load and where
/// to be, and asks it only about its own state, never about the room's.
/// Its events report what its own user did in it, and what became of a
/// load.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MediaPlayerTrait: std::fmt::Debug + Send {
    fn start(&mut self);
    fn pause(&mut self);
    fn is_paused(&self) -> Option<bool>;
    fn set_speed(&mut self, speed: f64);
    /// The room is at `pos` now, and the player is to be there with it. A
    /// playing player aims ahead by what the room plays until it lands.
    fn set_position(&mut self, pos: Duration);
    /// Where the player is in what it has loaded. It has no position
    /// while it holds nothing, is still loading or is on its way to
    /// another position.
    fn get_position(&mut self) -> Option<Duration>;
    /// Loads `source` paused at `pos`, in place of what the player holds.
    fn load(&mut self, source: Source, pos: Duration);
    fn unload(&mut self);
    async fn event(&mut self) -> MediaPlayerEvent;
}

/// What a player plays a video from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Url(String),
    /// The file `provider` offers, read from that peer chunk by chunk
    /// through a [`crate::stream::Reader`].
    Stream {
        provider: PeerId,
        file: SharedFile,
    },
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum MediaPlayerEvent {
    Pause(PlayerPause),
    Start(PlayerStart),
    CachePause(PlayerCachePause),
    PositionChange(PlayerPositionChange),
    SpeedChange(PlayerSpeedChange),
    Loaded(PlayerLoaded),
    FileEnd(PlayerFileEnd),
    Failed(PlayerFailed),
    Exit(PlayerExit),
}

/// The user paused in the player, which makes them not ready.
#[derive(Debug, Clone)]
pub struct PlayerPause;

impl EventHandler for PlayerPause {
    fn handle(self, model: &mut CoreModel) {
        trace!("player pause");
        model.ready = false;
        model.ui.user_update(crate::ui::UserChange {
            name: model.config.username.clone(),
            ready: false,
        });
        model.request(Intent::Pause);
    }
}

/// The user started playback in the player, which makes them ready.
#[derive(Debug, Clone)]
pub struct PlayerStart;

impl EventHandler for PlayerStart {
    fn handle(self, model: &mut CoreModel) {
        trace!("player start");
        model.ready = true;
        model.ui.user_update(crate::ui::UserChange {
            name: model.config.username.clone(),
            ready: true,
        });
        model.request(Intent::Start);
    }
}

/// The player waits for its cache to fill, and the room with it. The user
/// stays as ready as before.
#[derive(Debug, Clone)]
pub struct PlayerCachePause;

impl EventHandler for PlayerCachePause {
    fn handle(self, model: &mut CoreModel) {
        trace!("player cache pause");
        model.request(Intent::CachePause);
    }
}

/// The player is at `pos` without the room having moved there: its user
/// seeked.
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
        let Some(video) = model.followed.player.clone() else {
            return;
        };
        let position = self.pos;
        model.request(Intent::Seek { video, position });
    }
}

/// The user changed the speed in the player, which plays at it already.
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
        model.followed.speed = self.speed;
        model.request(Intent::Speed(self.speed));
    }
}

/// The player finished loading its video and has a position from now on.
/// The room may have moved on while it loaded, so the position is brought
/// in line exactly, and the room learns that this user has the video.
#[derive(Debug, Clone)]
pub struct PlayerLoaded;

impl EventHandler for PlayerLoaded {
    fn handle(self, model: &mut CoreModel) {
        trace!("player loaded its video");
        if model.followed.player.is_none() {
            return;
        }
        model.followed.loaded = true;
        model.followed.jumps = None;
        model.follow(Instant::now());
        model.publish_status();
    }
}

/// The player reached the end of what it had loaded and holds nothing
/// anymore.
#[derive(Debug, Clone)]
pub struct PlayerFileEnd;

impl EventHandler for PlayerFileEnd {
    fn handle(self, model: &mut CoreModel) {
        trace!("player file end");
        model.followed.source = None;
        model.followed.loaded = false;
        if let Some(video) = model.followed.player.take() {
            model.request(Intent::Ended(video));
        }
    }
}

/// The player could not play what it was given, or not any further: a
/// source it can not open, or a stream whose chunks stopped coming. It
/// holds nothing anymore. The video did not end, so the room stays where
/// it is, and this user has the video no longer.
#[derive(Debug, Clone)]
pub struct PlayerFailed;

impl EventHandler for PlayerFailed {
    fn handle(self, model: &mut CoreModel) {
        trace!("player failed");
        let Some(source) = model.followed.source.take() else {
            return;
        };
        let message = match source {
            Source::Stream { .. } => "The stream of the video failed",
            Source::File(_) | Source::Url(_) => "The player could not play the video",
        };
        model.ui.player_message(PlayerMessage::error(message));
        if std::mem::take(&mut model.followed.loaded) {
            model.publish_status();
        }
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
