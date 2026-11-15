use config::Config;
use directories::ProjectDirs;
use enum_dispatch::enum_dispatch;
use futures::future::OptionFuture;
use logging::ChatLogger;
use once_cell::sync::Lazy;
use player::wrapper::MediaPlayerWrapper;
use playlist::Video;
use playlist::handler::PlaylistHandler;
use room::{Followed, Role, RoomName, RoomState};
use tracing::{info, trace};
use video_provider::VideoProviderTrait;

use self::communicator::*;
use self::file_database::*;
use self::heartbeat::Pacemaker;
use self::player::*;
use self::ui::*;
use self::video_provider::*;
use self::video_server::*;

pub mod builder;
pub mod chunk;
pub mod communicator;
pub mod config;
pub mod file_database;
pub mod fuzzy;
pub mod heartbeat;
pub mod logging;
pub mod player;
pub mod playlist;
pub mod room;
pub mod sync;
pub mod ui;
pub mod user;
pub mod util;
pub mod video_provider;
pub mod video_server;

pub static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("de", "autumnal", "niketsu"));

#[enum_dispatch]
pub trait EventHandler {
    fn handle(self, model: &mut CoreModel);
}

/// Where a host's room starts from when this core has no state for the room
/// yet: the most recently saved playlist of the room, if any.
pub type SavedPlaylistLoader = fn(&RoomName) -> Option<PlaylistHandler>;

/// Where this core persists the playlist of a room after every change.
pub type PlaylistSaver = fn(&RoomName, &PlaylistHandler);

#[derive(Debug)]
pub struct CoreModel {
    pub communicator: Box<dyn CommunicatorTrait>,
    pub player: MediaPlayerWrapper,
    pub ui: Box<dyn UserInterfaceTrait>,
    pub database: Box<dyn FileDatabaseTrait>,
    pub video_server: Box<dyn VideoServerTrait>,
    pub video_provider: Box<dyn VideoProviderTrait>,
    chat_logger: Option<ChatLogger>,
    pub config: Config,
    pub ready: bool,
    pub running: bool,
    /// The room's state: the only word on its playlist, selection, play
    /// state, position and users.
    pub state: RoomState,
    /// Whether this core decides the state or takes it from a host.
    pub role: Role,
    pub(crate) followed: Followed,
    pub(crate) saved_playlist: SavedPlaylistLoader,
    pub(crate) save_playlist: PlaylistSaver,
}

impl CoreModel {
    /// Persist the room's playlist and selection for the configured room.
    pub fn save_playlist(&self) {
        let playlist = self.state.playlist.clone();
        let saved = PlaylistHandler::new(playlist, self.state.video.as_ref());
        (self.save_playlist)(&self.config.room, &saved);
    }

    /// Connects to the configured room, leaving the one connected to.
    pub fn connect(&mut self) {
        self.communicator.connect(self.config.endpoint());
    }

    /// Stops providing the current file and tells the UI and the room.
    pub fn stop_sharing(&mut self) {
        self.video_provider.stop_providing();
        self.ui.video_share(false);
        self.publish_status();
    }

    /// Carry auto-sharing over to the newly selected `video` if we are
    /// currently sharing and have the file; otherwise stop sharing. The
    /// room learns of a carried share once its file is open, and of the
    /// offer withdrawn until then from this user's status.
    pub(crate) fn reshare_or_stop(&mut self, video: Option<&Video>) {
        let offered = self.video_provider.offered();
        let file = video
            .filter(|_| self.config.auto_share && self.video_provider.sharing())
            .and_then(|video| self.database.find_file(video.as_str()));
        match file {
            Some(file) => self.video_provider.start_providing(file),
            None => self.video_provider.stop_providing(),
        }
        self.ui.video_share(self.video_provider.sharing());
        if self.video_provider.offered() != offered {
            self.publish_status();
        }
    }
}

#[derive(Debug)]
pub struct Core {
    pub model: CoreModel,
}

impl Core {
    pub async fn run(mut self) {
        info!("starting core");
        if self.model.config.auto_connect {
            info!("connecting to the configured room");
            self.model.connect();
        }
        self.run_loop().await;
    }

    pub async fn run_loop(mut self) {
        info!("enter main loop");
        let mut pacemaker = Pacemaker::default();
        while self.model.running {
            tokio::select! {
                com = self.model.communicator.receive() => {
                    trace!("handle communicator event");
                    com.handle(&mut self.model);
                }
                play = self.model.player.event() => {
                    trace!("handle player event");
                    play.handle(&mut self.model);
                }
                ui = self.model.ui.event() => {
                    trace!("handle ui event");
                    ui.handle(&mut self.model);
                }
                video_server = self.model.video_server.event() => {
                    trace!("handle video server event");
                    video_server.handle(&mut self.model);
                }
                video_provider = self.model.video_provider.event() => {
                    trace!("handle video provider event");
                    video_provider.handle(&mut self.model);
                }
                beat = pacemaker.recv() => {
                    trace!("handle pacemaker event");
                    beat.handle(&mut self.model);
                }
                Some(db) = self.model.database.event() => {
                    trace!("handle database event");
                    db.handle(&mut self.model);
                }
                Some(message) = OptionFuture::from(self.model.chat_logger.as_mut().map(ChatLogger::recv)) => {
                    self.model.ui.player_message(message)
                }
            }
        }
    }
}

#[macro_export]
macro_rules! log_err {
    ($result:expr) => {
        if let Err(error) = $result {
            tracing::error!(%error);
        }
    };
    ($result:expr, $default:expr) => {
        match $result {
            Ok(ok_val) => ok_val,
            Err(error) => {
                tracing::error!(%error);
                $default
            }
        }
    };
}

#[macro_export]
macro_rules! log_err_msg {
    ($result:expr, $msg:expr) => {
        if let Err(error) = $result {
            tracing::error!(%error, $msg);
        }
    };
    ($result:expr, $default:expr, $msg:expr) => {
        match $result {
            Ok(ok_val) => ok_val,
            Err(error) => {
                tracing::error!(%error, $msg);
                $default
            }
        }
    };
}
