use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Local};
use enum_dispatch::enum_dispatch;
use log::{trace, Level};
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::sync::Notify;

use super::communicator::{
    EndpointInfo, NiketsuJoin, NiketsuPlaylist, NiketsuSelect, NiketsuUserMessage,
};
use super::playlist::Video;
use super::user::UserStatus;
use super::{CoreModel, EventHandler};
use crate::config::Config;
use crate::file_database::FileStore;
use crate::playlist::Playlist;
use crate::rooms::RoomList;
use crate::util::{Observed, RingBuffer};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait UserInterfaceTrait: std::fmt::Debug + Send {
    fn file_database_status(&mut self, update_status: f32);
    fn file_database(&mut self, db: FileStore);
    fn playlist(&mut self, playlist: Playlist);
    fn video_change(&mut self, video: Option<Video>);
    fn room_list(&mut self, room_list: RoomList);
    fn user_update(&mut self, user: UserChange);
    fn player_message(&mut self, msg: PlayerMessage);
    fn username_change(&mut self, username: String);

    async fn event(&mut self) -> UserInterfaceEvent;
}

#[enum_dispatch(EventHandler)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserInterfaceEvent {
    PlaylistChange,
    VideoChange,
    ServerChange,
    RoomChange,
    UserChange,
    UserMessage,
    FileDatabaseChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistChange {
    pub playlist: Playlist,
}

impl EventHandler for PlaylistChange {
    fn handle(self, model: &mut CoreModel) {
        trace!("playlist change message");
        let actor = model.config.username.clone();
        let playlist = self.playlist.iter().map(|v| v.as_str().into()).collect();

        model.playlist.replace(self.playlist);
        model
            .communicator
            .send(NiketsuPlaylist { actor, playlist }.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoChange {
    pub video: Video,
}

impl EventHandler for VideoChange {
    fn handle(self, model: &mut CoreModel) {
        trace!("video change message");
        let actor = model.config.username.clone();
        let filename = Some(self.video.as_str().to_string());
        let position = Duration::ZERO;
        model.playlist.select_playing(&self.video);
        model.communicator.send(
            NiketsuSelect {
                actor,
                filename,
                position,
            }
            .into(),
        );

        model
            .player
            .load_video(self.video, Duration::ZERO, model.database.all_files());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        trace!("server change message");
        model.config.password = self.password.clone().unwrap_or_default();
        model.communicator.connect(self.clone().into());
        self.room.handle(model);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        trace!("room change message");
        let room = self.room;
        let username = model.config.username.clone();
        let password = model.config.password.clone();
        model.config.room = room.clone();
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserChange {
    pub name: String,
    pub ready: bool,
}

impl EventHandler for UserChange {
    fn handle(self, model: &mut CoreModel) {
        trace!("user change message");
        model.config.username = self.name;
        model.ready = self.ready;
        model
            .communicator
            .send(model.config.status(model.ready).into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessage {
    pub message: String,
}

impl EventHandler for UserMessage {
    fn handle(self, model: &mut CoreModel) {
        trace!("user chat message");
        let actor = model.config.username.clone();
        let message = self.message;
        model
            .communicator
            .send(NiketsuUserMessage { actor, message }.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerMessageInner {
    pub message: String,
    pub source: MessageSource,
    pub level: MessageLevel,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageSource {
    UserMessage(String),
    UserAction(String),
    Server,
    Internal,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MessageLevel {
    Normal,
    Success,
    Warn,
    Error,
    Debug,
    Trace,
}

impl From<Level> for MessageLevel {
    fn from(value: Level) -> Self {
        match value {
            Level::Error => MessageLevel::Error,
            Level::Warn => MessageLevel::Warn,
            Level::Info => MessageLevel::Success,
            Level::Debug => MessageLevel::Debug,
            Level::Trace => MessageLevel::Trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDatabaseChange {
    ChangePaths(Vec<PathBuf>),
    StartUpdate,
    StopUpdate,
}

impl EventHandler for FileDatabaseChange {
    fn handle(self, model: &mut CoreModel) {
        match self {
            FileDatabaseChange::ChangePaths(paths) => {
                trace!("filedatabase change paths message");
                model.database.clear_paths();
                for path in paths {
                    model.database.add_path(path);
                }
                model.database.start_update();
            }
            FileDatabaseChange::StartUpdate => {
                trace!("filedatabase start update message");
                model.database.start_update()
            }

            FileDatabaseChange::StopUpdate => {
                trace!("filedatabase stop update message");
                model.database.stop_update()
            }
        }
    }
}

#[derive(Debug)]
pub struct UserInterface {
    model: UiModel,
    ui_events: MpscReceiver<UserInterfaceEvent>,
}

impl UserInterface {
    pub fn new(config: &Config) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let user = UserStatus {
            name: config.username.clone(),
            ready: false,
        };
        let model = UiModel {
            file_database: Observed::<_>::default_with_notify(&notify),
            file_database_status: Observed::<_>::default_with_notify(&notify),
            playlist: Observed::<_>::default_with_notify(&notify),
            playing_video: Observed::<_>::default_with_notify(&notify),
            room_list: Observed::<_>::default_with_notify(&notify),
            user: Observed::<_>::new(user, &notify),
            messages: Observed::new(RingBuffer::new(1000), &notify),
            events: tx,
            notify,
        };
        Self {
            model,
            ui_events: rx,
        }
    }

    pub fn model(&self) -> &UiModel {
        &self.model
    }
}

#[async_trait]
impl UserInterfaceTrait for UserInterface {
    fn file_database_status(&mut self, update_status: f32) {
        self.model.file_database_status.set(update_status);
    }

    fn file_database(&mut self, db: FileStore) {
        self.model.file_database.set(db);
    }

    fn playlist(&mut self, playlist: Playlist) {
        self.model.playlist.set(playlist);
    }

    fn video_change(&mut self, video: Option<Video>) {
        self.model.playing_video.set(video);
    }

    fn room_list(&mut self, room_list: RoomList) {
        self.model.room_list.set(room_list);
    }

    fn user_update(&mut self, user: UserChange) {
        self.model.user.set(user.into());
    }

    fn player_message(&mut self, msg: PlayerMessage) {
        self.model.messages.rcu(|msgs| {
            let mut msgs = RingBuffer::clone(msgs);
            msgs.push(msg.clone());
            msgs
        });
    }

    fn username_change(&mut self, username: String) {
        let mut user = self.model.user.get_inner();
        user.name = username;
        self.model.user.set(user)
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        self.ui_events.recv().await.expect("ui event stream ended")
    }
}

#[derive(Clone, Debug)]
pub struct UiModel {
    pub file_database: Observed<FileStore>,
    pub file_database_status: Observed<f32>,
    pub playlist: Observed<Playlist>,
    pub playing_video: Observed<Option<Video>>,
    pub room_list: Observed<RoomList>,
    pub user: Observed<UserStatus>,
    pub messages: Observed<RingBuffer<PlayerMessage>>,
    pub events: MpscSender<UserInterfaceEvent>,
    pub notify: Arc<Notify>,
}

impl UiModel {
    pub fn user_ready_toggle(&self) {
        trace!("toggle user ready");
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
        trace!("change username");
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
        trace!("send user message");
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
        trace!("change db paths");
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::ChangePaths(paths),
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn start_db_update(&self) {
        trace!("start db update");
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::StartUpdate,
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn stop_db_update(&self) {
        trace!("stop db update");
        let res = self
            .events
            .send(UserInterfaceEvent::FileDatabaseChange(
                FileDatabaseChange::StopUpdate,
            ))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_room(&self, request: RoomChange) {
        trace!("change room");
        let res = self
            .events
            .send(UserInterfaceEvent::RoomChange(request))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_server(&self, request: ServerChange) {
        trace!("change server");
        let res = self
            .events
            .send(UserInterfaceEvent::ServerChange(request))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_video(&self, video: Video) {
        trace!("change video");
        self.playing_video.set(Some(video.clone()));
        let res = self
            .events
            .send(UserInterfaceEvent::VideoChange(VideoChange { video }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }

    pub fn change_playlist(&self, playlist: Playlist) {
        trace!("change playlist");
        let res = self
            .events
            .send(UserInterfaceEvent::PlaylistChange(PlaylistChange {
                playlist,
            }))
            .map_err(anyhow::Error::from);
        crate::log!(res)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arcstr::ArcStr;
    use mockall::predicate::{always, eq};
    use tokio::sync::Notify;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::{MockCommunicatorTrait, NiketsuUserStatus, OutgoingMessage};
    use crate::config::Config;
    use crate::file_database::{FileEntry, MockFileDatabaseTrait};
    use crate::player::MockMediaPlayerTrait;
    use crate::playlist::MockPlaylistHandlerTrait;
    use crate::util::Observed;

    #[test]
    fn test_playlist_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let mut playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let videos: [ArcStr; 2] = ["video1".into(), "video2".into()];
        let playlist = Playlist::from_iter(videos.iter());
        let config = Config {
            username: user.clone(),
            ..Default::default()
        };
        let message = OutgoingMessage::from(NiketsuPlaylist {
            actor: user.clone(),
            playlist: videos.into_iter().collect(),
        });

        playlist_handler
            .expect_replace()
            .with(eq(playlist.clone()))
            .once()
            .return_const(());
        communicator
            .expect_send()
            .with(eq(message))
            .once()
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .playlist(Box::new(playlist_handler))
            .file_database(Box::new(file_database))
            .config(config)
            .build();

        let change = PlaylistChange { playlist };
        change.handle(&mut core.model)
    }

    #[test]
    fn test_video_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let mut player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let mut file_database = MockFileDatabaseTrait::default();
        let mut playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let video = Video::from("video1");
        let file = FileEntry::new("video1".into(), "/video1".into(), None);
        let file_store = FileStore::from_iter([file.clone()]);
        let pos = Duration::ZERO;
        let config = Config {
            username: user.clone(),
            ..Default::default()
        };
        let message = OutgoingMessage::from(NiketsuSelect {
            actor: user.clone(),
            filename: Some("video1".to_string()),
            position: pos,
        });

        file_database.expect_all_files().return_const(file_store);
        player.expect_get_speed().return_const(1.1);
        playlist_handler
            .expect_select_playing()
            .with(eq(video.clone()))
            .once()
            .return_const(());
        player
            .expect_load_video()
            .with(eq(video.clone()), eq(pos), always())
            .once()
            .return_const(());
        communicator
            .expect_send()
            .with(eq(message))
            .once()
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let change = VideoChange { video };
        change.handle(&mut core.model)
    }

    #[test]
    fn test_server_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let addr = String::from("duckduckgo.com");
        let secure = true;
        let password = Some(String::from("passwd"));
        let room: RoomChange = String::from("room1").into();
        let config = Config {
            username: user.clone(),
            ..Default::default()
        };
        let endpoint = EndpointInfo {
            addr: addr.clone(),
            secure,
        };
        let message = OutgoingMessage::from(NiketsuJoin {
            password: password.clone().unwrap(),
            room: room.room.clone(),
            username: user.clone(),
        });

        communicator
            .expect_connect()
            .once()
            .with(eq(endpoint))
            .return_const(());
        communicator
            .expect_send()
            .once()
            .with(eq(message))
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let change = ServerChange {
            addr,
            secure,
            password,
            room: room.clone(),
        };
        change.handle(&mut core.model);

        assert_eq!(core.model.config.room, room.room);
    }

    #[test]
    fn test_room_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let password = String::from("passwd");
        let room = String::from("room1");
        let config = Config {
            username: user.clone(),
            password: password.clone(),
            ..Default::default()
        };
        let message = OutgoingMessage::from(NiketsuJoin {
            password: password.clone(),
            room: room.clone(),
            username: user.clone(),
        });

        communicator
            .expect_send()
            .once()
            .with(eq(message))
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let change = RoomChange { room: room.clone() };
        change.handle(&mut core.model);

        assert_eq!(core.model.config.room, room);
    }

    #[test]
    fn test_user_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let user_new = String::from("moritz");
        let ready = true;
        let config = Config {
            username: user.clone(),
            ..Default::default()
        };
        let message = OutgoingMessage::from(NiketsuUserStatus {
            ready,
            username: user_new.clone(),
        });

        communicator
            .expect_send()
            .once()
            .with(eq(message))
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        assert_eq!(core.model.config.username, user);
        assert!(!core.model.ready);

        let change = UserChange {
            name: user_new.clone(),
            ready,
        };
        change.handle(&mut core.model);

        assert_eq!(core.model.config.username, user_new);
        assert_eq!(core.model.ready, ready);
    }

    #[test]
    fn test_user_message() {
        let mut communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let user = String::from("max");
        let user_msg = String::from("hello world!");
        let config = Config {
            username: user.clone(),
            ..Default::default()
        };
        let message = OutgoingMessage::from(NiketsuUserMessage {
            actor: user.clone(),
            message: user_msg.clone(),
        });

        communicator
            .expect_send()
            .once()
            .with(eq(message))
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let change = UserMessage { message: user_msg };
        change.handle(&mut core.model);
    }

    #[test]
    fn test_file_database_change_start_stop() {
        let communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let mut file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let config = Config::default();

        file_database.expect_start_update().once().return_const(());
        file_database.expect_stop_update().once().return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let start_update = FileDatabaseChange::StartUpdate;
        let stop_update = FileDatabaseChange::StopUpdate;

        start_update.handle(&mut core.model);
        stop_update.handle(&mut core.model);
    }

    #[test]
    fn test_file_database_change_paths() {
        let communicator = MockCommunicatorTrait::default();
        let player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let mut file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let paths = vec!["/videos".into(), "/music".into()];
        let paths_clone = paths.clone();
        let config = Config::default();

        file_database.expect_clear_paths().once().return_const(());
        file_database
            .expect_add_path()
            .times(2)
            .withf(move |f| paths_clone.contains(f))
            .return_const(());
        file_database.expect_start_update().once().return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
            .config(config)
            .build();

        let change = FileDatabaseChange::ChangePaths(paths.to_vec());

        change.handle(&mut core.model);
    }

    #[test]
    fn test_user_ready_toggle() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let user = UserStatus {
            name: "TestUser".to_string(),
            ready: false,
        };
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(user, &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: Arc::new(Notify::new()),
        };

        ui_model.user_ready_toggle();

        let user = ui_model.user.get_inner();
        assert!(user.ready);
    }

    #[test]
    fn test_change_username() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let user = UserStatus {
            name: "TestUser".to_string(),
            ready: false,
        };
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(user, &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: Arc::new(Notify::new()),
        };

        ui_model.change_username("NewName".to_string());

        let user = ui_model.user.get_inner();
        assert_eq!(user.name, "NewName");
    }

    #[test]
    fn test_send_message() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let user = UserStatus {
            name: "TestUser".to_string(),
            ready: false,
        };
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(user.clone(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        ui_model.send_message("Hello, world!".to_string());

        let messages = ui_model.messages.get_inner();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages.get(0).unwrap().message, "Hello, world!");
    }

    #[test]
    fn test_change_db_paths() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let paths = vec![
            PathBuf::from("/path/to/file1"),
            PathBuf::from("/path/to/file2"),
        ];
        let event = FileDatabaseChange::ChangePaths(paths.clone()).into();

        ui_model.change_db_paths(paths.clone());

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, event);
    }

    #[test]
    fn test_start_db_update() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let event = FileDatabaseChange::StartUpdate.into();

        ui_model.start_db_update();

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, event);
    }

    #[test]
    fn test_stop_db_update() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let event = FileDatabaseChange::StopUpdate.into();

        ui_model.stop_db_update();

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, event);
    }

    #[test]
    fn test_change_room() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let room = String::from("room1");
        let request = RoomChange { room: room.clone() };

        ui_model.change_room(request.clone());

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, request.into());
    }

    #[test]
    fn test_change_server() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let addr = String::from("duckduckgo.com");
        let secure = true;
        let password = Some(String::from("passwd"));
        let room: RoomChange = String::from("room1").into();
        let request = ServerChange {
            addr,
            secure,
            password,
            room,
        };
        ui_model.change_server(request.clone());

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, request.into());
    }

    #[test]
    fn test_change_video() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let video = Video::from("video1");
        let request = VideoChange {
            video: video.clone(),
        };
        ui_model.change_video(video);

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, request.into());
    }

    #[test]
    fn test_change_playlist() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let ui_model = UiModel {
            file_database: Observed::new(FileStore::default(), &notify),
            file_database_status: Observed::new(0.0, &notify),
            playlist: Observed::new(Playlist::default(), &notify),
            playing_video: Observed::new(None, &notify),
            room_list: Observed::new(RoomList::default(), &notify),
            user: Observed::new(UserStatus::default(), &notify),
            messages: Observed::new(RingBuffer::new(10), &notify),
            events: tx,
            notify: notify.clone(),
        };

        let playlist = Playlist::from_iter(["video1"]);
        let request = PlaylistChange {
            playlist: playlist.clone(),
        };
        ui_model.change_playlist(playlist);

        let received_event = rx.try_recv().unwrap();
        assert_eq!(received_event, request.into());
    }
}
