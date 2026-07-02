use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::communicator::{
    CommunicatorTrait, ConnectedMsg, EndpointInfo, IncomingMessage, OutgoingMessage, PlaylistMsg,
    SelectMsg, UserStatusListMsg, VideoProviderStoppedMsg,
};
use niketsu_core::config::Config;
use niketsu_core::file_database::{
    FileDatabaseEvent, FileDatabaseTrait, FileEntry, FilePathSearch, FileStore,
};
use niketsu_core::heartbeat::Heartbeat;
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait};
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use niketsu_core::ui::{PlayerMessage, UserChange, UserInterfaceEvent, UserInterfaceTrait};
use niketsu_core::user::UserStatus;
use niketsu_core::video_provider::{VideoProviderEvent, VideoProviderTrait};
use niketsu_core::video_server::{VideoServerEvent, VideoServerTrait};
use niketsu_core::{CoreModel, EventHandler};

// ------------------------------------------------------------------
// Fake player: stands in for mpv. State is shared with the test via a
// handle so scenarios can simulate playback (position, slow loads).
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub video: Option<Video>,
    pub position: Duration,
    pub speed: f64,
    pub paused: bool,
    pub file_loaded: bool,
    /// When true, `load_video` leaves `file_loaded` false until the test
    /// flips it — simulates mpv still opening the file.
    pub load_lag: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            video: None,
            position: Duration::ZERO,
            speed: 1.0,
            paused: true,
            file_loaded: false,
            load_lag: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerHandle(Arc<Mutex<PlayerState>>);

impl PlayerHandle {
    pub fn state(&self) -> PlayerState {
        self.0.lock().unwrap().clone()
    }

    pub fn set(&self, f: impl FnOnce(&mut PlayerState)) {
        f(&mut self.0.lock().unwrap())
    }
}

#[derive(Debug)]
struct FakePlayer(Arc<Mutex<PlayerState>>);

impl FakePlayer {
    fn new() -> (Self, PlayerHandle) {
        let state = Arc::new(Mutex::new(PlayerState::default()));
        (Self(state.clone()), PlayerHandle(state))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PlayerState> {
        self.0.lock().unwrap()
    }
}

#[async_trait]
impl MediaPlayerTrait for FakePlayer {
    fn start(&mut self) {
        self.lock().paused = false;
    }

    fn pause(&mut self) {
        self.lock().paused = true;
    }

    fn is_paused(&self) -> Option<bool> {
        let state = self.lock();
        state.video.as_ref().map(|_| state.paused)
    }

    fn set_speed(&mut self, speed: f64) {
        self.lock().speed = speed;
    }

    fn get_speed(&self) -> f64 {
        self.lock().speed
    }

    fn set_position(&mut self, pos: Duration) {
        self.lock().position = pos;
    }

    fn get_position(&mut self) -> Option<Duration> {
        let state = self.lock();
        state.video.as_ref().map(|_| state.position)
    }

    fn cache_available(&mut self) -> bool {
        false
    }

    fn load_video(&mut self, load: Video, pos: Duration, _db: &FileStore) {
        let mut state = self.lock();
        state.video = Some(load);
        state.position = pos;
        state.file_loaded = !state.load_lag;
    }

    fn unload_video(&mut self) {
        let mut state = self.lock();
        state.video = None;
        state.file_loaded = false;
    }

    fn maybe_reload_video(&mut self, _f: &dyn FilePathSearch) {}

    fn reload_video(&mut self, _f: &dyn FilePathSearch, _filename: &str) {}

    fn playing_video(&self) -> Option<Video> {
        self.lock().video.clone()
    }

    fn video_loaded(&self) -> bool {
        self.lock().file_loaded
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Fake UI: records what the core pushed to it.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub playing_video: Option<Video>,
    pub video_share: bool,
    pub is_host: bool,
}

#[derive(Debug, Clone)]
pub struct UiHandle(Arc<Mutex<UiState>>);

impl UiHandle {
    pub fn state(&self) -> UiState {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Debug)]
struct FakeUi(Arc<Mutex<UiState>>);

impl FakeUi {
    fn new() -> (Self, UiHandle) {
        let state = Arc::new(Mutex::new(UiState::default()));
        (Self(state.clone()), UiHandle(state))
    }
}

#[async_trait]
impl UserInterfaceTrait for FakeUi {
    fn file_database_status(&mut self, _update_status: f32) {}
    fn file_database(&mut self, _db: FileStore) {}
    fn playlist(&mut self, _playlist: Playlist) {}

    fn video_change(&mut self, video: Option<Video>) {
        self.0.lock().unwrap().playing_video = video;
    }

    fn user_list(&mut self, _user_list: UserList) {}
    fn user_update(&mut self, _user: UserChange) {}
    fn player_message(&mut self, _msg: PlayerMessage) {}
    fn username_change(&mut self, _username: ArcStr) {}
    fn abort(&mut self) {}

    fn video_share(&mut self, video_share: bool) {
        self.0.lock().unwrap().video_share = video_share;
    }

    fn is_host(&mut self, is_host: bool) {
        self.0.lock().unwrap().is_host = is_host;
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Fake communicator: outgoing messages pile up in an outbox that the
// Room drains and routes. Nothing is ever received asynchronously —
// the Room injects IncomingMessages by calling handlers directly.
// ------------------------------------------------------------------

type Outbox = Arc<Mutex<Vec<OutgoingMessage>>>;

#[derive(Debug, Default)]
struct FakeCommunicator {
    outbox: Outbox,
}

#[async_trait]
impl CommunicatorTrait for FakeCommunicator {
    fn connect(&mut self, _connect: EndpointInfo) {}

    fn send(&mut self, msg: OutgoingMessage) {
        self.outbox.lock().unwrap().push(msg);
    }

    async fn receive(&mut self) -> IncomingMessage {
        std::future::pending().await
    }

    fn has_endpoint(&self) -> bool {
        true
    }
}

// ------------------------------------------------------------------
// Fake file database.
// ------------------------------------------------------------------

#[derive(Debug, Default)]
struct FakeDatabase {
    entries: Vec<FileEntry>,
    store: FileStore,
}

impl FakeDatabase {
    fn new(files: &[&str]) -> Self {
        let entries: Vec<_> = files
            .iter()
            .map(|name| FileEntry::new(name.to_string(), format!("/{name}").into(), None))
            .collect();
        let store = FileStore::from_iter(entries.clone());
        Self { entries, store }
    }
}

#[async_trait]
impl FileDatabaseTrait for FakeDatabase {
    fn add_path(&mut self, _path: PathBuf) {}
    fn del_path(&mut self, _path: &Path) {}
    fn clear_paths(&mut self) {}

    fn get_paths(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn start_update(&mut self) {}
    fn stop_update(&mut self) {}

    fn find_file(&self, filename: &str) -> Option<FileEntry> {
        self.entries
            .iter()
            .find(|e| e.file_name() == filename)
            .cloned()
    }

    fn all_files(&self) -> &FileStore {
        &self.store
    }

    async fn event(&mut self) -> Option<FileDatabaseEvent> {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Fake video server: records what it serves and which chunks arrived.
// ------------------------------------------------------------------

#[derive(Debug, Default)]
struct ServerState {
    running: Option<ArcStr>,
    chunks: Vec<(u64, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub struct ServerHandle(Arc<Mutex<ServerState>>);

impl ServerHandle {
    pub fn running(&self) -> bool {
        self.0.lock().unwrap().running.is_some()
    }

    pub fn chunks(&self) -> Vec<(u64, Vec<u8>)> {
        self.0.lock().unwrap().chunks.clone()
    }
}

#[derive(Debug)]
struct FakeVideoServer(Arc<Mutex<ServerState>>);

impl FakeVideoServer {
    fn new() -> (Self, ServerHandle) {
        let state = Arc::new(Mutex::new(ServerState::default()));
        (Self(state.clone()), ServerHandle(state))
    }
}

#[async_trait]
impl VideoServerTrait for FakeVideoServer {
    fn stop_server(&mut self) {
        self.0.lock().unwrap().running = None;
    }

    fn start_server(&mut self, file_name: ArcStr, _file_size: u64) {
        let mut state = self.0.lock().unwrap();
        state.running = Some(file_name);
        state.chunks.clear();
    }

    fn insert_chunk(&mut self, _file_name: &str, start: u64, bytes: Vec<u8>) {
        self.0.lock().unwrap().chunks.push((start, bytes));
    }

    fn addr(&self) -> Option<SocketAddr> {
        None
    }

    async fn event(&mut self) -> VideoServerEvent {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Fake video provider: records which chunks the core asked it to read
// so tests can answer them with ChunkResponse events.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ChunkRequested {
    pub uuid: uuid::Uuid,
    pub start: u64,
    pub len: u64,
}

#[derive(Debug, Default)]
struct ProviderState {
    providing: Option<ArcStr>,
    requests: Vec<ChunkRequested>,
}

#[derive(Debug, Clone)]
pub struct ProviderHandle(Arc<Mutex<ProviderState>>);

impl ProviderHandle {
    pub fn requests(&self) -> Vec<ChunkRequested> {
        self.0.lock().unwrap().requests.clone()
    }
}

#[derive(Debug)]
struct FakeVideoProvider(Arc<Mutex<ProviderState>>);

impl FakeVideoProvider {
    fn new() -> (Self, ProviderHandle) {
        let state = Arc::new(Mutex::new(ProviderState::default()));
        (Self(state.clone()), ProviderHandle(state))
    }
}

#[async_trait]
impl VideoProviderTrait for FakeVideoProvider {
    fn start_providing(&mut self, file: FileEntry) {
        self.0.lock().unwrap().providing = Some(file.file_name_arc());
    }

    fn stop_providing(&mut self) {
        self.0.lock().unwrap().providing = None;
    }

    fn request_chunk(&mut self, uuid: uuid::Uuid, file_name: &str, start: u64, len: u64) {
        let mut state = self.0.lock().unwrap();
        if state.providing.as_deref() == Some(file_name) {
            state.requests.push(ChunkRequested { uuid, start, len });
        }
    }

    fn size(&self) -> Option<u64> {
        self.0.lock().unwrap().providing.as_ref().map(|_| 1_000)
    }

    fn sharing(&self) -> bool {
        self.0.lock().unwrap().providing.is_some()
    }

    fn file_name(&self) -> Option<ArcStr> {
        self.0.lock().unwrap().providing.clone()
    }

    async fn event(&mut self) -> VideoProviderEvent {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Peer: one full core (model + fakes) plus the test-side handles.
// ------------------------------------------------------------------

pub struct Peer {
    pub model: CoreModel,
    pub player: PlayerHandle,
    pub ui: UiHandle,
    pub video_server: ServerHandle,
    pub provider: ProviderHandle,
    name: ArcStr,
    outbox: Outbox,
}

impl Peer {
    fn new(name: &str, files: &[&str]) -> Self {
        let (player, player_handle) = FakePlayer::new();
        let (ui, ui_handle) = FakeUi::new();
        let (video_server, server_handle) = FakeVideoServer::new();
        let (video_provider, provider_handle) = FakeVideoProvider::new();
        let communicator = FakeCommunicator::default();
        let outbox = communicator.outbox.clone();
        let config = Config {
            username: ArcStr::from(name),
            room: ArcStr::from("dance"),
            auto_share: false,
            ..Default::default()
        };
        let core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(FakeDatabase::new(files)))
            .video_server(Box::new(video_server))
            .video_provider(Box::new(video_provider))
            .config(config)
            .build();
        Self {
            model: core.model,
            player: player_handle,
            ui: ui_handle,
            video_server: server_handle,
            provider: provider_handle,
            name: ArcStr::from(name),
            outbox,
        }
    }

    pub fn name(&self) -> ArcStr {
        self.name.clone()
    }

    /// Perform a local action, e.g. a UI event or a player event.
    pub fn act(&mut self, event: impl EventHandler) {
        event.handle(&mut self.model)
    }

    pub fn heartbeat(&mut self) {
        Heartbeat.handle(&mut self.model)
    }

    pub fn playlist(&self) -> Playlist {
        self.model.playlist.get_playlist()
    }

    fn drain(&mut self) -> Vec<OutgoingMessage> {
        std::mem::take(&mut self.outbox.lock().unwrap())
    }
}

// ------------------------------------------------------------------
// Room: the in-memory relay. Owns the routing rules of the dance.
// ------------------------------------------------------------------

#[derive(Default)]
pub struct Room {
    pub peers: Vec<Peer>,
    host: usize,
    statuses: BTreeSet<UserStatus>,
    provider: Option<usize>,
    /// Room state the host replays to late joiners (`send_init_status`);
    /// `select.position` is kept fresh from the host's video status.
    playlist: Option<PlaylistMsg>,
    select: Option<SelectMsg>,
    /// uuid → requester, for routing responses back like the direct
    /// request-response channels in `p2p/file_share.rs` do.
    file_requests: HashMap<uuid::Uuid, usize>,
    chunk_requests: HashMap<uuid::Uuid, usize>,
}

impl Room {
    pub fn new() -> Self {
        Self::default()
    }

    /// The first peer to join is the host, as with the real relay. Later
    /// joiners are greeted with the room's playlist and current selection,
    /// mirroring the host's `send_init_status`.
    pub fn join(&mut self, name: &str, files: &[&str]) -> usize {
        let mut peer = Peer::new(name, files);
        let is_host = self.peers.is_empty();
        ConnectedMsg { is_host }.handle(&mut peer.model);
        if let Some(playlist) = &self.playlist {
            playlist.clone().handle(&mut peer.model);
        }
        if let Some(select) = &self.select {
            select.clone().handle(&mut peer.model);
        }
        self.peers.push(peer);
        self.peers.len() - 1
    }

    pub fn peer(&mut self, index: usize) -> &mut Peer {
        &mut self.peers[index]
    }

    pub fn host_index(&self) -> usize {
        self.host
    }

    /// Deliver messages until the room is quiescent. Panicking on too
    /// many rounds is the no-echo rule: no handler may re-emit the
    /// broadcast it is reacting to, or the room never settles.
    pub fn pump(&mut self) {
        for _ in 0..100 {
            let mut deliveries = Vec::new();
            for from in 0..self.peers.len() {
                for msg in self.peers[from].drain() {
                    deliveries.extend(self.route(from, msg));
                }
            }
            if deliveries.is_empty() {
                return;
            }
            for (to, msg) in deliveries {
                msg.handle(&mut self.peers[to].model);
            }
        }
        panic!("message storm: the room never went quiescent (no-echo rule violated?)");
    }

    /// The relay's routing rules, mirroring `client/communicator`:
    /// - only the host's VideoStatus is a valid reference (clients drop
    ///   their own and reject non-host status)
    /// - user statuses are aggregated into a broadcast status list
    /// - file requests and chunks travel directly between requester and
    ///   the provider announced via VideoShareChange (kademlia + direct
    ///   request-response in the real network)
    /// - a host handover re-auths both sides at the relay
    /// - everything else is broadcast
    fn route(&mut self, from: usize, msg: OutgoingMessage) -> Vec<(usize, IncomingMessage)> {
        let others = |m: IncomingMessage| -> Vec<(usize, IncomingMessage)> {
            (0..self.peers.len())
                .filter(|to| *to != from)
                .map(|to| (to, m.clone()))
                .collect()
        };
        match msg {
            OutgoingMessage::VideoStatus(m) if from == self.host => {
                if let Some(select) = &mut self.select {
                    select.position = m.position.unwrap_or_default();
                }
                others(IncomingMessage::VideoStatus(m))
            }
            OutgoingMessage::VideoStatus(_) => vec![],
            OutgoingMessage::Start(m) => others(IncomingMessage::Start(m)),
            OutgoingMessage::Pause(m) => others(IncomingMessage::Pause(m)),
            OutgoingMessage::PlaybackSpeed(m) => others(IncomingMessage::PlaybackSpeed(m)),
            OutgoingMessage::Seek(m) => others(IncomingMessage::Seek(m)),
            OutgoingMessage::Select(m) => {
                self.select = Some(m.clone());
                others(IncomingMessage::Select(m))
            }
            OutgoingMessage::UserMessage(m) => others(IncomingMessage::UserMessage(m)),
            OutgoingMessage::Playlist(m) => {
                self.playlist = Some(m.clone());
                others(IncomingMessage::Playlist(m))
            }
            OutgoingMessage::UserStatus(m) => {
                self.statuses.retain(|s| s.name != m.name);
                self.statuses.insert(m);
                let list = UserStatusListMsg {
                    room_name: ArcStr::from("dance"),
                    users: self.statuses.clone(),
                };
                (0..self.peers.len())
                    .map(|to| (to, IncomingMessage::UserStatusList(list.clone())))
                    .collect()
            }
            OutgoingMessage::HostHandover(m) if from == self.host => {
                let Some(target) = self.peers.iter().position(|p| p.name == m.new_host) else {
                    return vec![];
                };
                let old = self.host;
                self.host = target;
                let mut deliveries = others(IncomingMessage::HostHandover(m));
                // the relay re-auths both sides: the target reconnects as
                // the host, the old host rejoins as a client
                deliveries.push((
                    target,
                    IncomingMessage::Connected(ConnectedMsg { is_host: true }),
                ));
                deliveries.push((
                    old,
                    IncomingMessage::Connected(ConnectedMsg { is_host: false }),
                ));
                deliveries
            }
            OutgoingMessage::HostHandover(_) => vec![],
            OutgoingMessage::VideoShareChange(m) => match m.video {
                Some(_) => {
                    self.provider = Some(from);
                    vec![]
                }
                None if self.provider == Some(from) => {
                    self.provider = None;
                    others(IncomingMessage::VideoProviderStopped(
                        VideoProviderStoppedMsg,
                    ))
                }
                None => vec![],
            },
            OutgoingMessage::FileRequest(m) => match self.provider {
                Some(provider) if provider != from => {
                    self.file_requests.insert(m.uuid, from);
                    vec![(provider, IncomingMessage::FileRequest(m))]
                }
                // kademlia finds no providers: the requester's consumer
                // gives up and tells its own core the provider is gone
                _ => vec![(
                    from,
                    IncomingMessage::VideoProviderStopped(VideoProviderStoppedMsg),
                )],
            },
            OutgoingMessage::FileResponse(m) => match self.file_requests.remove(&m.uuid) {
                Some(requester) => vec![(requester, IncomingMessage::FileResponse(m))],
                None => vec![],
            },
            OutgoingMessage::ChunkRequest(m) => match self.provider {
                Some(provider) if provider != from => {
                    self.chunk_requests.insert(m.uuid, from);
                    vec![(provider, IncomingMessage::ChunkRequest(m))]
                }
                _ => vec![(
                    from,
                    IncomingMessage::VideoProviderStopped(VideoProviderStoppedMsg),
                )],
            },
            OutgoingMessage::ChunkResponse(m) => match self.chunk_requests.remove(&m.uuid) {
                Some(requester) => vec![(requester, IncomingMessage::ChunkResponse(m))],
                None => vec![],
            },
        }
    }
}
