use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::communicator::{
    CommunicatorTrait, ConnectedMsg, EndpointInfo, HostHandoverMsg, HostRoundTripMsg,
    IncomingMessage, OutgoingMessage, PeerConnectedMsg, PeerDisconnectedMsg, PeerId, PeerStatusMsg,
    VideoProviderStoppedMsg,
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
    pub playlist: Playlist,
    pub playing_video: Option<Video>,
    pub video_share: bool,
    pub is_host: bool,
    pub users: UserList,
}

impl UiState {
    /// The user list as shown, name and ready flag per user.
    pub fn user_names(&self) -> Vec<(String, bool)> {
        self.users
            .iter()
            .map(|u| (u.name.to_string(), u.ready))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct UiHandle(Arc<Mutex<UiState>>);

impl UiHandle {
    pub fn state(&self) -> UiState {
        self.0.lock().unwrap().clone()
    }

    /// Mirrors the optimistic local set the real `UiModel` performs for
    /// its own edits (e.g. `change_playlist`) before the event reaches
    /// the core.
    pub fn set(&self, f: impl FnOnce(&mut UiState)) {
        f(&mut self.0.lock().unwrap())
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

    fn playlist(&mut self, playlist: Playlist) {
        self.0.lock().unwrap().playlist = playlist;
    }

    fn video_change(&mut self, video: Option<Video>) {
        self.0.lock().unwrap().playing_video = video;
    }

    fn user_list(&mut self, user_list: UserList) {
        self.0.lock().unwrap().users = user_list;
    }
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
    /// The transport identity the communicator reports for this peer.
    pub id: PeerId,
    /// False once the peer left the room; it then neither sends nor
    /// receives anything.
    pub online: bool,
    outbox: Outbox,
}

/// A valid peer id without key material (an identity multihash).
fn peer_id(index: usize) -> PeerId {
    let [a, b, c, d] = (index as u32).to_be_bytes();
    PeerId::from_bytes(&[0x00, 4, a, b, c, d]).unwrap()
}

impl Peer {
    fn new(index: usize, name: &str, files: &[&str], auto_share: bool) -> Self {
        let (player, player_handle) = FakePlayer::new();
        let (ui, ui_handle) = FakeUi::new();
        let (video_server, server_handle) = FakeVideoServer::new();
        let (video_provider, provider_handle) = FakeVideoProvider::new();
        let communicator = FakeCommunicator::default();
        let outbox = communicator.outbox.clone();
        let config = Config {
            username: ArcStr::from(name),
            room: ArcStr::from("dance"),
            auto_share,
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
            // every room in the dance starts empty, independent of what
            // earlier runs saved
            .saved_playlist(|_| None)
            .build();
        Self {
            model: core.model,
            player: player_handle,
            ui: ui_handle,
            video_server: server_handle,
            provider: provider_handle,
            id: peer_id(index),
            online: true,
            outbox,
        }
    }

    /// The name this peer currently goes by, which the host may have
    /// forced.
    pub fn name(&self) -> ArcStr {
        self.model.config.username.clone()
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

/// The network between the cores. It only transports: every room decision
/// (who is in the room, what late joiners get, auto-start, renames) is made
/// by the host's core, exactly as with the real communicator.
#[derive(Default)]
pub struct Room {
    pub peers: Vec<Peer>,
    host: usize,
    provider: Option<usize>,
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
    /// joiners connect to the host, whose core greets them.
    pub fn join(&mut self, name: &str, files: &[&str]) -> usize {
        let index = self.peers.len();
        self.admit(Peer::new(index, name, files, false))
    }

    /// Like [`Room::join`], but the peer carries its share over to new
    /// selections (`auto_share = true` in its config).
    pub fn join_with_auto_share(&mut self, name: &str, files: &[&str]) -> usize {
        let index = self.peers.len();
        self.admit(Peer::new(index, name, files, true))
    }

    fn admit(&mut self, peer: Peer) -> usize {
        let index = self.peers.len();
        self.peers.push(peer);
        if index == 0 {
            self.host = index;
            self.deliver(index, ConnectedMsg { is_host: true }.into());
        } else {
            self.connect_client(index);
        }
        index
    }

    /// A client's direct connection to the host comes up: the host's
    /// communicator reports the peer, the client's reports the connection.
    fn connect_client(&mut self, index: usize) {
        let peer = self.peers[index].id;
        self.deliver(self.host, PeerConnectedMsg { peer }.into());
        self.deliver(index, ConnectedMsg { is_host: false }.into());
    }

    fn deliver(&mut self, to: usize, msg: IncomingMessage) {
        msg.handle(&mut self.peers[to].model)
    }

    fn index_of(&self, id: &PeerId) -> Option<usize> {
        self.peers.iter().position(|p| p.id == *id && p.online)
    }

    /// The peer's connections close; the host's communicator reports it.
    pub fn leave(&mut self, index: usize) {
        assert_ne!(index, self.host, "the dance has no host loss");
        self.peers[index].online = false;
        self.peers[index].drain();
        let peer = self.peers[index].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer }.into());
    }

    /// A ping to the host answered after `rtt`.
    pub fn round_trip(&mut self, index: usize, rtt: Duration) {
        self.deliver(index, HostRoundTripMsg { rtt }.into());
    }

    /// Every peer's pacemaker ticks once, clients first so the host's
    /// heartbeat is judged against their fresh local state.
    pub fn heartbeat(&mut self) {
        for i in 0..self.peers.len() {
            if self.peers[i].online && i != self.host {
                self.peers[i].heartbeat();
            }
        }
        let host = self.host;
        self.peers[host].heartbeat();
    }

    pub fn peer(&mut self, index: usize) -> &mut Peer {
        &mut self.peers[index]
    }

    pub fn host_index(&self) -> usize {
        self.host
    }

    /// The provider currently announced to the room, if any.
    pub fn provider_index(&self) -> Option<usize> {
        self.provider
    }

    /// Deliver messages until the room is quiescent. Panicking on too
    /// many rounds is the no-echo rule: no handler may re-emit the
    /// broadcast it is reacting to, or the room never settles.
    pub fn pump(&mut self) {
        for _ in 0..100 {
            let mut deliveries = Vec::new();
            for from in 0..self.peers.len() {
                let outbox = self.peers[from].drain();
                if !self.peers[from].online {
                    continue;
                }
                for msg in outbox {
                    deliveries.extend(self.route(from, msg));
                }
            }
            if deliveries.is_empty() {
                return;
            }
            for (to, msg) in deliveries {
                if self.peers[to].online {
                    self.deliver(to, msg);
                }
            }
        }
        panic!("message storm: the room never went quiescent (no-echo rule violated?)");
    }

    /// The transport rules of `client/communicator`:
    /// - gossip reaches every other peer; a VideoStatus is only accepted
    ///   when its signed source is the host
    /// - a client's playlist and status are requests to the host; the
    ///   host's playlist and user list are gossip
    /// - `ToPeer` is a direct request to exactly that peer
    /// - file requests and chunks travel directly between requester and
    ///   the provider announced via VideoShareChange (kademlia + direct
    ///   request-response in the real network)
    /// - a host handover moves the room at the relay: the target comes
    ///   back as the host and everyone else reconnects to it as a client
    fn route(&mut self, from: usize, msg: OutgoingMessage) -> Vec<(usize, IncomingMessage)> {
        let others = |m: IncomingMessage| -> Vec<(usize, IncomingMessage)> {
            (0..self.peers.len())
                .filter(|to| *to != from && self.peers[*to].online)
                .map(|to| (to, m.clone()))
                .collect()
        };
        let from_host = from == self.host;
        match msg {
            OutgoingMessage::VideoStatus(m) if from_host => others(m.into()),
            OutgoingMessage::VideoStatus(_) => vec![],
            OutgoingMessage::Start(m) => others(m.into()),
            OutgoingMessage::Pause(m) => others(m.into()),
            OutgoingMessage::PlaybackSpeed(m) => others(m.into()),
            OutgoingMessage::Seek(m) => others(m.into()),
            OutgoingMessage::Select(m) => others(m.into()),
            OutgoingMessage::UserMessage(m) => others(m.into()),
            OutgoingMessage::Playlist(m) if from_host => others(m.into()),
            OutgoingMessage::Playlist(m) => vec![(self.host, m.into())],
            OutgoingMessage::UserStatus(_) if from_host => vec![],
            OutgoingMessage::UserStatus(status) => {
                let peer = self.peers[from].id;
                vec![(self.host, PeerStatusMsg { peer, status }.into())]
            }
            OutgoingMessage::UserStatusList(m) if from_host => others(m.into()),
            OutgoingMessage::UserStatusList(_) => vec![],
            OutgoingMessage::ToPeer(m) if from_host => match self.index_of(&m.peer) {
                Some(to) => vec![(to, m.message.into())],
                None => vec![],
            },
            OutgoingMessage::ToPeer(_) => vec![],
            OutgoingMessage::HostHandover(m) if from_host => {
                let Some(target) = self.index_of(&m.peer) else {
                    return vec![];
                };
                self.host = target;
                let new_host = m.new_host;
                let mut deliveries = others(HostHandoverMsg { new_host }.into());
                deliveries.push((target, ConnectedMsg { is_host: true }.into()));
                for i in 0..self.peers.len() {
                    if i == target || !self.peers[i].online {
                        continue;
                    }
                    let peer = self.peers[i].id;
                    deliveries.push((target, PeerConnectedMsg { peer }.into()));
                    deliveries.push((i, ConnectedMsg { is_host: false }.into()));
                }
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
