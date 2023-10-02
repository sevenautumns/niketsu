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

#[cfg_attr(test, mockall::automock)]
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
        debug!("Playlist Change Message");
        let actor = model.user.name.clone();
        let playlist = self.playlist.iter().map(|v| v.as_str().into()).collect();

        model.playlist.replace(self.playlist);
        model
            .communicator
            .send(NiketsuPlaylist { actor, playlist }.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        debug!("Server Change Message");
        model.password = self.password.clone();
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use arcstr::ArcStr;
    use mockall::predicate::eq;
    use tokio::sync::Notify;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::{MockCommunicatorTrait, NiketsuUserStatus, OutgoingMessage};
    use crate::file_database::{FileEntry, MockFileDatabaseTrait};
    use crate::player::{LoadVideo, MockMediaPlayerTrait, PlayerVideo};
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
            .username(user)
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
        let video = PlaylistVideo::from("video1");
        let file = FileEntry::new("video1".into(), "/video1".into(), None);
        let file_store = FileStore::from_iter([file.clone()]);
        let message = OutgoingMessage::from(NiketsuSelect {
            actor: user.clone(),
            filename: Some("video1".to_string()),
        });
        let load = LoadVideo {
            video: PlayerVideo::File(file),
            pos: Duration::ZERO,
            speed: 1.1,
            paused: true,
        };

        file_database.expect_all_files().return_const(file_store);
        player.expect_get_speed().return_const(1.1);
        playlist_handler
            .expect_select_playing()
            .with(eq(video.clone()))
            .once()
            .return_const(());
        player
            .expect_load_video()
            .with(eq(load))
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
            .username(user)
            .playlist(Box::new(playlist_handler))
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
            .username(user)
            .playlist(Box::new(playlist_handler))
            .build();

        let change = ServerChange {
            addr,
            secure,
            password,
            room: room.clone(),
        };
        change.handle(&mut core.model);

        assert_eq!(core.model.room, room.room);
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
            .username(user)
            .password(password)
            .playlist(Box::new(playlist_handler))
            .build();

        let change = RoomChange { room: room.clone() };
        change.handle(&mut core.model);

        assert_eq!(core.model.room, room);
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
            .username(user.clone())
            .playlist(Box::new(playlist_handler))
            .build();

        assert_eq!(core.model.user.name, user);
        assert!(!core.model.user.ready);

        let change = UserChange {
            name: user_new.clone(),
            ready,
        };
        change.handle(&mut core.model);

        assert_eq!(core.model.user.name, user_new);
        assert_eq!(core.model.user.ready, ready);
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
            .username(user.clone())
            .playlist(Box::new(playlist_handler))
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

        file_database.expect_start_update().once().return_const(());
        file_database.expect_stop_update().once().return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(file_database))
            .playlist(Box::new(playlist_handler))
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

        let video = PlaylistVideo::from("video1");
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
