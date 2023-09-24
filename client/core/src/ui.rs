use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Local};
use enum_dispatch::enum_dispatch;
use log::debug;
use tokio::sync::mpsc::UnboundedSender as MpscSender;
use tokio::sync::Notify;

use super::communicator::{
    EndpointInfo, NiketsuJoin, NiketsuPlaylist, NiketsuSelect, NiketsuUserMessage,
};
use super::playlist::PlaylistVideo;
use super::user::UserStatus;
use super::{CoreModel, EventHandler};
use crate::file_database::FileStore;
use crate::playlist::Playlist;
use crate::rooms::RoomList;
use crate::util::{Observed, RingBuffer};
use crate::MediaPlayerTraitExt;

#[async_trait]
pub trait UserInterfaceTrait: std::fmt::Debug + Send {
    fn file_database_status(&mut self, update_status: f32);
    fn file_database(&mut self, db: FileStore);
    fn playlist(&mut self, playlist: Playlist);
    fn video_change(&mut self, video: Option<PlaylistVideo>);
    fn room_list(&mut self, room_list: RoomList);
    fn user_update(&mut self, user: UserChange);
    fn player_message(&mut self, msg: PlayerMessage);

    async fn event(&mut self) -> UserInterfaceEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone)]
pub enum UserInterfaceEvent {
    PlaylistChange,
    VideoChange,
    ServerChange,
    RoomChange,
    UserChange,
    UserMessage,
    FileDatabaseChange,
}

#[derive(Debug, Clone)]
pub struct PlaylistChange {
    pub playlist: Playlist,
}

impl EventHandler for PlaylistChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("Playlist Change Message");
        let actor = model.user.name.clone();
        let playlist = self.playlist.iter().map(|v| v.as_str().into()).collect();

        model.playlist.replace(self.playlist);
        model
            .communicator
            .send(NiketsuPlaylist { actor, playlist }.into())
    }
}

#[derive(Debug, Clone)]
pub struct VideoChange {
    pub video: PlaylistVideo,
}

impl EventHandler for VideoChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("Video Change Message");
        let actor = model.user.name.clone();
        let filename = Some(self.video.as_str().to_string());
        model.playlist.select_playing(&self.video);
        model
            .communicator
            .send(NiketsuSelect { actor, filename }.into());

        model
            .player
            .load_playlist_video(&self.video, model.database.all_files());
    }
}

#[derive(Debug, Clone)]
pub struct ServerChange {
    pub addr: String,
    pub secure: bool,
    pub password: Option<String>,
    pub room: RoomChange,
}

impl From<ServerChange> for EndpointInfo {
    fn from(value: ServerChange) -> Self {
        Self {
            addr: value.addr,
            secure: value.secure,
        }
    }
}

impl EventHandler for ServerChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("Server Change Message");
        model.password = self.password.clone();
        model.communicator.connect(self.clone().into());
        self.room.handle(model);
    }
}

#[derive(Debug, Clone)]
pub struct RoomChange {
    pub room: String,
}

impl From<String> for RoomChange {
    fn from(value: String) -> Self {
        Self { room: value }
    }
}

impl EventHandler for RoomChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("Room Change Message");
        let room = self.room;
        let username = model.user.name.clone();
        let password = model.password.clone().unwrap_or_default();
        model.room = room.clone();
        model.communicator.send(
            NiketsuJoin {
                password,
                room,
                username,
            }
            .into(),
        );
    }
}

#[derive(Debug, Clone)]
pub struct UserChange {
    pub name: String,
    pub ready: bool,
}

impl EventHandler for UserChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("User Change Message");
        model.user.name = self.name;
        model.user.ready = self.ready;
        model.communicator.send(model.user.clone().into());
    }
}

#[derive(Debug, Clone)]
pub struct UserMessage {
    pub message: String,
}

impl EventHandler for UserMessage {
    fn handle(self, model: &mut CoreModel) {
        debug!("User Chat Message");
        let actor = model.user.name.clone();
        let message = self.message;
        model
            .communicator
            .send(NiketsuUserMessage { actor, message }.into())
    }
}

#[derive(Debug, Clone)]
pub struct PlayerMessage {
    inner: Arc<PlayerMessageInner>,
}

impl Deref for PlayerMessage {
    type Target = PlayerMessageInner;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl From<PlayerMessageInner> for PlayerMessage {
    fn from(value: PlayerMessageInner) -> Self {
        let inner = Arc::new(value);
        Self { inner }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerMessageInner {
    pub message: String,
    pub source: MessageSource,
    pub level: MessageLevel,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum MessageSource {
    UserMessage(String),
    UserAction(String),
    Server,
    Internal,
}

#[derive(Debug, Copy, Clone)]
pub enum MessageLevel {
    Normal,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub enum FileDatabaseChange {
    ChangePaths(Vec<PathBuf>),
    StartUpdate,
    StopUpdate,
}

impl EventHandler for FileDatabaseChange {
    fn handle(self, model: &mut CoreModel) {
        debug!("Filedatabase Change Message");
        match self {
            FileDatabaseChange::ChangePaths(paths) => {
                model.database.clear_paths();
                for path in paths {
                    model.database.add_path(path);
                }
                model.database.start_update();
            }
            FileDatabaseChange::StartUpdate => model.database.start_update(),
            FileDatabaseChange::StopUpdate => model.database.stop_update(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct UiModel {
    pub file_database: Observed<FileStore>,
    pub file_database_status: Observed<f32>,
    pub playlist: Observed<Playlist>,
    pub playing_video: Observed<Option<PlaylistVideo>>,
    pub room_list: Observed<RoomList>,
    pub user: Observed<UserStatus>,
    pub messages: Observed<RingBuffer<PlayerMessage>>,
    pub events: MpscSender<UserInterfaceEvent>,
    pub notify: Arc<Notify>,
}

impl UiModel {
    pub fn user_ready_toggle(&self) {
        let mut user = self.user.get_inner();
        user.ready = !user.ready;
        self.user.set(user.clone());
        let res = self
            .events
            .send(UserInterfaceEvent::UserChange(UserChange {
                name: user.name.clone(),
                ready: user.ready,
            }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_username(&self, name: String) {
        let mut user = self.user.get_inner();
        user.name = name;
        self.user.set(user.clone());
        let res = self
            .events
            .send(UserInterfaceEvent::UserChange(UserChange {
                name: user.name.clone(),
                ready: user.ready,
            }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn send_message(&self, msg: String) {
        let player_message: PlayerMessage = PlayerMessageInner {
            message: msg.clone(),
            source: MessageSource::UserMessage(self.user.get_inner_arc().name.clone()),
            level: MessageLevel::Normal,
            timestamp: Local::now(),
        }
        .into();
        self.messages.rcu(|msgs| {
            let mut msgs = RingBuffer::clone(msgs);
            msgs.push(player_message.clone());
            msgs
        });
        let res = self
            .events
            .send(UserInterfaceEvent::UserMessage(UserMessage {
                message: msg,
            }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_db_paths(&self, paths: Vec<PathBuf>) {
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::ChangePaths(paths),
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn start_db_update(&self) {
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::StartUpdate,
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn stop_db_update(&self) {
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::StopUpdate,
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_room(&self, request: RoomChange) {
        let res = self
            .events
            .send(UserInterfaceEvent::RoomChange(request))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_server(&self, request: ServerChange) {
        let res = self
            .events
            .send(UserInterfaceEvent::ServerChange(request))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_video(&self, video: PlaylistVideo) {
        self.playing_video.set(Some(video.clone()));
        let res = self
            .events
            .send(UserInterfaceEvent::VideoChange(VideoChange { video }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_playlist(&self, playlist: Playlist) {
        let res = self
            .events
            .send(UserInterfaceEvent::PlaylistChange(PlaylistChange {
                playlist,
            }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }
}
