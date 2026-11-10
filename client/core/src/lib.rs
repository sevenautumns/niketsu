use std::time::Duration;

use arcstr::ArcStr;
use config::Config;
use directories::ProjectDirs;
use enum_dispatch::enum_dispatch;
use futures::future::OptionFuture;
use logging::ChatLogger;
use once_cell::sync::Lazy;
use player::wrapper::MediaPlayerWrapper;
use playlist::file::PlaylistBrowser;
use playlist::handler::PlaylistHandler;
use playlist::{Playlist, Video};
use room::{HostRoom, RoomName, UserList, select_next};
use sync::ClientSync;
use tracing::{debug, info, trace};
use video_provider::VideoProviderTrait;

use self::communicator::*;
use self::file_database::*;
use self::heartbeat::Pacemaker;
use self::player::*;
use self::ui::*;
use self::video_provider::*;
use self::video_server::*;

pub mod builder;
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

#[derive(Debug)]
pub struct CoreModel {
    pub communicator: Box<dyn CommunicatorTrait>,
    pub player: MediaPlayerWrapper,
    pub ui: Box<dyn UserInterfaceTrait>,
    pub database: Box<dyn FileDatabaseTrait>,
    pub video_server: Box<dyn VideoServerTrait>,
    pub video_provider: Box<dyn VideoProviderTrait>,
    pub playlist: PlaylistHandler,
    chat_logger: Option<ChatLogger>,
    pub config: Config,
    pub ready: bool,
    pub running: bool,
    /// The authoritative room state, present while this core is the host.
    pub host: Option<HostRoom>,
    /// How to follow the host's heartbeats while this core is a client.
    pub sync: ClientSync,
    /// The room the in-memory playlist and selection belong to, once this
    /// core has been connected to it.
    pub joined_room: Option<RoomName>,
    saved_playlist: SavedPlaylistLoader,
}

impl CoreModel {
    pub fn is_host(&self) -> bool {
        self.host.is_some()
    }

    /// Takes over the room. A core that already followed this room keeps
    /// what it has — playlist, running video and position — so a handover
    /// or a lost host does not interrupt playback. Otherwise the room starts
    /// from its saved playlist.
    pub(crate) fn become_host(&mut self) {
        let host_actor = arcstr::literal!("host");
        let select = if self.joined_room.as_ref() == Some(&self.config.room) {
            SelectMsg {
                actor: host_actor.clone(),
                position: self.player.get_position().unwrap_or_default(),
                video: self.player.playing_video(),
            }
        } else {
            let saved = (self.saved_playlist)(&self.config.room).unwrap_or_default();
            let select = SelectMsg {
                actor: host_actor.clone(),
                position: Duration::ZERO,
                video: saved.get_current_video(),
            };
            self.apply_playlist(PlaylistMsg {
                actor: host_actor.clone(),
                playlist: saved.get_playlist(),
            });
            self.apply_select(select.clone());
            select
        };
        let playlist = PlaylistMsg {
            actor: host_actor,
            playlist: self.playlist.get_playlist(),
        };
        self.host = Some(HostRoom::new(self.config.room.clone(), playlist, select));
    }

    /// Shows a playlist that arrived from the room (or was restored).
    pub(crate) fn apply_playlist(&mut self, msg: PlaylistMsg) {
        self.replace_playlist(msg.playlist.clone());
        self.ui.playlist(msg.playlist.clone());
        self.ui.player_message(PlayerMessage::from(msg));
    }

    /// Plays a selection that arrived from the room (or was restored).
    pub(crate) fn apply_select(&mut self, msg: SelectMsg) {
        self.select_video(msg.video.as_ref(), msg.position);
        self.ui.player_message(PlayerMessage::from(msg));
    }

    /// Announces our own status: a client tells the host, the host updates
    /// the room directly.
    pub fn publish_status(&mut self) {
        let status = self.config.status(self.ready);
        let Some(host) = &mut self.host else {
            self.communicator.send(status.into());
            return;
        };
        let actor = status.name.clone();
        host.users.update_local(status);
        self.start_if_all_ready(actor);
        self.publish_user_list();
    }

    /// Our own playlist edit, already applied locally.
    pub fn publish_playlist(&mut self, msg: PlaylistMsg) {
        if self.host.is_none() {
            self.communicator.send(msg.into());
            return;
        }
        self.host_adopt_playlist(msg.clone());
        self.communicator.send(msg.into());
    }

    /// Our own selection, already applied locally.
    pub fn publish_select(&mut self, msg: SelectMsg) {
        self.communicator.send(msg.clone().into());
        if let Some(host) = &mut self.host {
            host.select = msg;
            let actor = self.config.username.clone();
            self.start_if_all_ready(actor);
        }
    }

    /// The host takes `msg` as the room's playlist. If the playlist lost the
    /// selected video, the host moves the room on to the one that took its
    /// place.
    pub(crate) fn host_adopt_playlist(&mut self, msg: PlaylistMsg) {
        let Some(host) = &mut self.host else {
            return;
        };
        let next = select_next(&host.playlist, &host.select, &msg);
        let actor = msg.actor.clone();
        host.playlist = msg;
        let Some(select) = next else {
            return;
        };
        host.select = select.clone();
        self.apply_select(select.clone());
        self.communicator.send(select.into());
        self.start_if_all_ready(actor);
    }

    /// The host starts playback for the whole room once every user is
    /// ready.
    pub(crate) fn start_if_all_ready(&mut self, actor: ArcStr) {
        if !self
            .host
            .as_ref()
            .is_some_and(|host| host.users.all_ready())
        {
            return;
        }
        debug!("All users are ready. Starting playback for the room");
        let start = StartMsg { actor };
        self.communicator.send(start.clone().into());
        start.handle(self);
    }

    /// The host shows the room's user list and sends it to the room.
    pub(crate) fn publish_user_list(&mut self) {
        let Some(host) = &self.host else {
            return;
        };
        let list = host.users.status_list().clone();
        self.ui.user_list(UserList::from(list.clone()));
        self.communicator.send(list.into());
    }

    /// Persist the current playlist for the active room.
    pub fn save_playlist(&self) {
        PlaylistBrowser::save(&self.config.room, &self.playlist);
    }

    /// Replace the playlist and re-derive the playing marker from the
    /// player's video, so the marker is a function of the replicated state
    /// (playlist + running video) and not of the order edits happened in —
    /// late joiners derive it exactly this way from the replayed state.
    pub fn replace_playlist(&mut self, playlist: Playlist) {
        self.playlist.replace(playlist);
        if let Some(video) = self.player.playing_video() {
            self.playlist.select_playing(&video);
        }
        self.save_playlist();
    }

    /// Stop providing the current file and announce it to the room and UI.
    pub fn stop_sharing(&mut self) {
        self.communicator.send(VideoShareMsg { video: None }.into());
        self.video_provider.stop_providing();
        self.ui.video_share(false);
    }

    /// Switch playlist, player, sharing, persistence and UI over to `video` —
    /// the choreography shared by every video selection, local or remote.
    pub fn select_video(&mut self, video: Option<&Video>, position: Duration) {
        match video {
            Some(video) => {
                self.playlist.select_playing(video);
                let store = self.database.all_files();
                self.player.load_video(video.clone(), position, store);
                self.reshare_or_stop(video);
            }
            None => {
                self.playlist.unload_playing();
                self.player.unload_video();
                self.stop_sharing();
            }
        }
        self.save_playlist();
        self.ui.video_change(video.cloned());
    }

    /// Carry auto-sharing over to the newly selected `video` if we are
    /// currently sharing and have the file; otherwise stop sharing.
    fn reshare_or_stop(&mut self, video: &Video) {
        if self.config.auto_share
            && self.video_provider.sharing()
            && let Some(file) = self.database.find_file(video.as_str())
        {
            self.video_provider.start_providing(file);
            self.communicator
                .send(VideoShareMsg::new(video.clone()).into());
            self.ui.video_share(true);
        } else {
            self.stop_sharing();
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
            info!("autoconnect to server");
            self.auto_connect().await;
        }
        self.run_loop().await;
    }

    pub async fn auto_connect(&mut self) {
        let addr = self.model.config.addr();
        let room = self.model.config.room.clone();
        let password = self.model.config.password.clone();
        let endpoint = EndpointInfo {
            room: room.clone(),
            password,
            addr,
        };
        self.model.communicator.connect(endpoint);
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
