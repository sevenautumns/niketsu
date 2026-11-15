use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::chunk::SharedFile;
use niketsu_core::communicator::{
    ClientRequest, CommunicatorTrait, ConnectedMsg, EndpointInfo, HostInstruction,
    HostRoundTripMsg, IncomingMessage, NoticeMsg, OutgoingMessage, PeerConnectedMsg,
    PeerDisconnectedMsg, PeerId, PeerIntentMsg, RoomMessage, RoomStateMsg, StreamEndedMsg,
    StreamMsg,
};
use niketsu_core::config::Config;
use niketsu_core::file_database::{
    FileDatabaseEvent, FileDatabaseTrait, FileEntry, FilePathSearch, FileStore,
};
use niketsu_core::heartbeat::Heartbeat;
use niketsu_core::player::{MediaPlayerEvent, MediaPlayerTrait};
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use niketsu_core::ui::{
    MessageSource, PlayerMessage, UserChange, UserInterfaceEvent, UserInterfaceTrait,
};
use niketsu_core::video_provider::{VideoProviderEvent, VideoProviderTrait};
use niketsu_core::video_server::{VideoServerEvent, VideoServerTrait};
use niketsu_core::{CoreModel, EventHandler, SavedPlaylistLoader};

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
    /// When true, `set_position` only notes the target and the position
    /// stays until [`PlayerHandle::land_seek`] — simulates mpv seeking.
    pub seek_lag: bool,
    seek_target: Option<Duration>,
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
            seek_lag: false,
            seek_target: None,
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

    /// The player arrives where it was told to seek while `seek_lag` held
    /// it back.
    pub fn land_seek(&self) {
        let mut state = self.0.lock().unwrap();
        let target = state.seek_target.take().expect("no seek in flight");
        state.position = target;
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

    fn set_position(&mut self, pos: Duration) {
        let mut state = self.lock();
        match state.seek_lag {
            true => state.seek_target = Some(pos),
            false => state.position = pos,
        }
    }

    fn get_position(&mut self) -> Option<Duration> {
        let state = self.lock();
        state.video.as_ref().map(|_| state.position)
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
    /// The chat lines about what users did in the room, in order.
    pub actions: Vec<String>,
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
    fn player_message(&mut self, msg: PlayerMessage) {
        if let MessageSource::UserAction(_) = msg.source {
            self.0.lock().unwrap().actions.push(msg.message.clone());
        }
    }
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
// Fake video server: records what it serves.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ServerHandle(Arc<Mutex<Option<SharedFile>>>);

impl ServerHandle {
    pub fn running(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }

    /// The file the server serves, by name and size.
    pub fn serves(&self) -> Option<SharedFile> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Debug)]
struct FakeVideoServer(Arc<Mutex<Option<SharedFile>>>);

impl FakeVideoServer {
    fn new() -> (Self, ServerHandle) {
        let state = Arc::new(Mutex::new(None));
        (Self(state.clone()), ServerHandle(state))
    }
}

#[async_trait]
impl VideoServerTrait for FakeVideoServer {
    fn stop_server(&mut self) {
        *self.0.lock().unwrap() = None;
    }

    fn start_server(&mut self, name: ArcStr, size: u64) {
        *self.0.lock().unwrap() = Some(SharedFile { name, size });
    }

    async fn event(&mut self) -> VideoServerEvent {
        std::future::pending().await
    }
}

// ------------------------------------------------------------------
// Fake video provider: records which file it provides. It opens a file
// at once, so it offers what it provides; the test reports the file
// ready as the real provider's task would.
// ------------------------------------------------------------------

/// The size of every file a fake provider offers.
pub const FILE_SIZE: u64 = 1_000;

#[derive(Debug, Default)]
struct FakeVideoProvider(Option<ArcStr>);

#[async_trait]
impl VideoProviderTrait for FakeVideoProvider {
    fn start_providing(&mut self, file: FileEntry) {
        self.0 = Some(file.file_name_arc());
    }

    fn stop_providing(&mut self) {
        self.0 = None;
    }

    fn sharing(&self) -> bool {
        self.0.is_some()
    }

    fn file_name(&self) -> Option<ArcStr> {
        self.0.clone()
    }

    fn offered(&self) -> Option<SharedFile> {
        let name = self.0.clone()?;
        Some(SharedFile {
            name,
            size: FILE_SIZE,
        })
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
    /// The transport identity the communicator reports for this peer.
    pub id: PeerId,
    /// False once the peer left the room; it then neither sends nor
    /// receives anything.
    pub online: bool,
    outbox: Outbox,
    /// The room states that reached this peer, in the order they did.
    states: Vec<RoomStateMsg>,
}

/// A valid peer id without key material (an identity multihash).
fn peer_id(index: usize) -> PeerId {
    let [a, b, c, d] = (index as u32).to_be_bytes();
    PeerId::from_bytes(&[0x00, 4, a, b, c, d]).unwrap()
}

impl Peer {
    fn new(
        index: usize,
        name: &str,
        files: &[&str],
        auto_share: bool,
        saved: SavedPlaylistLoader,
    ) -> Self {
        let (player, player_handle) = FakePlayer::new();
        let (ui, ui_handle) = FakeUi::new();
        let (video_server, server_handle) = FakeVideoServer::new();
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
            .video_provider(Box::new(FakeVideoProvider::default()))
            .config(config)
            // what a room has saved is the Room's to say, independent of
            // what earlier runs saved
            .saved_playlist(saved)
            .save_playlist(|_, _| {})
            .build();
        Self {
            model: core.model,
            player: player_handle,
            ui: ui_handle,
            video_server: server_handle,
            id: peer_id(index),
            online: true,
            outbox,
            states: Vec::new(),
        }
    }

    /// The room states that reached this peer since the last call.
    pub fn take_states(&mut self) -> Vec<RoomStateMsg> {
        std::mem::take(&mut self.states)
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

    /// The host's core never handles what this client sent since the last
    /// pump. The communicator loses no message silently in either
    /// direction, see [`Room::fail_link`] and [`Room::fail_request`]; this
    /// stands for intents the host handles too late for the client to
    /// wait for them.
    pub fn lose_messages(&mut self) {
        self.drain();
    }

    fn drain(&mut self) -> Vec<OutgoingMessage> {
        std::mem::take(&mut self.outbox.lock().unwrap())
    }
}

// ------------------------------------------------------------------
// Room: the in-memory relay. Owns the routing rules of the dance.
// ------------------------------------------------------------------

/// What the host sends one member, as the outbox in
/// `client/communicator/src/p2p/host.rs` sends it: in order, one message
/// on its way at a time, a waiting room state giving way to a newer one,
/// and the playlist only until the member took it. Delivery is instant
/// unless the test holds the link.
#[derive(Debug, Default)]
struct Link {
    /// The `playlist_version` of the last playlist the member took.
    playlist: Option<u64>,
    /// While the test holds the link: the message on its way, then what
    /// waits behind it.
    held: Option<VecDeque<IncomingMessage>>,
}

impl Link {
    /// `msg` as it goes out now: a room state without the playlist the
    /// member took already.
    fn send(&mut self, mut msg: IncomingMessage) -> IncomingMessage {
        if let IncomingMessage::RoomState(state) = &mut msg {
            if self.playlist == Some(state.playlist_version) {
                state.playlist = None;
            } else if state.playlist.is_some() {
                self.playlist = Some(state.playlist_version);
            }
        }
        msg
    }

    /// Queues `msg` behind the one on its way, where a newer room state
    /// takes the place of a waiting one at the back of the queue.
    fn wait(queue: &mut VecDeque<IncomingMessage>, msg: IncomingMessage) {
        if matches!(msg, IncomingMessage::RoomState(_)) {
            let waiting = queue
                .iter()
                .skip(1)
                .position(|m| matches!(m, IncomingMessage::RoomState(_)));
            if let Some(i) = waiting {
                queue.remove(i + 1);
            }
        }
        queue.push_back(msg);
    }
}

/// The network between the cores. It only transports: every room decision
/// (who is in the room, what late joiners get, auto-start, renames) is made
/// by the host's core, exactly as with the real communicator.
#[derive(Default)]
pub struct Room {
    pub peers: Vec<Peer>,
    host: usize,
    /// The host's outbox to each member, by the member's index.
    links: HashMap<usize, Link>,
    /// Who streams from whom: the provider of each consumer, as its
    /// communicator keeps it in `p2p/file_share.rs`.
    streams: HashMap<usize, usize>,
    /// What each peer offered when the room last looked, to end the
    /// streams of an offer withdrawn.
    offers: HashMap<usize, SharedFile>,
    /// What a fresh host finds saved for the room; nothing unless set.
    saved: Option<SavedPlaylistLoader>,
}

impl Room {
    /// A room with nothing saved.
    pub fn new() -> Self {
        Self::default()
    }

    /// A room for which `saved` yields the playlist of an earlier session.
    pub fn with_saved_playlist(saved: SavedPlaylistLoader) -> Self {
        Self {
            saved: Some(saved),
            ..Self::default()
        }
    }

    fn saved(&self) -> SavedPlaylistLoader {
        self.saved.unwrap_or(|_| None)
    }

    /// The first peer to join is the host, as with the real relay. Later
    /// joiners connect to the host, whose core greets them.
    pub fn join(&mut self, name: &str, files: &[&str]) -> usize {
        let index = self.peers.len();
        self.admit(Peer::new(index, name, files, false, self.saved()))
    }

    /// Like [`Room::join`], but the peer carries its share over to new
    /// selections (`auto_share = true` in its config).
    pub fn join_with_auto_share(&mut self, name: &str, files: &[&str]) -> usize {
        let index = self.peers.len();
        self.admit(Peer::new(index, name, files, true, self.saved()))
    }

    fn admit(&mut self, peer: Peer) -> usize {
        let index = self.peers.len();
        self.peers.push(peer);
        if index == 0 {
            self.host = index;
            let peer = self.peers[index].id;
            self.deliver(
                index,
                ConnectedMsg {
                    is_host: true,
                    peer,
                },
            );
        } else {
            self.connect_client(index);
        }
        index
    }

    /// A client's direct connection to the host comes up: the host's
    /// communicator reports the peer, the client's reports the connection.
    fn connect_client(&mut self, index: usize) {
        let peer = self.peers[index].id;
        self.links.insert(index, Link::default());
        self.deliver(self.host, PeerConnectedMsg { peer });
        self.deliver(
            index,
            ConnectedMsg {
                is_host: false,
                peer,
            },
        );
    }

    fn index_of(&self, id: &PeerId) -> Option<usize> {
        self.peers.iter().position(|p| p.id == *id && p.online)
    }

    /// The host loses the room at the relay, and the room there closes
    /// with it. Everyone connects again; `target` is the first to arrive
    /// and hosts the room, the others — the old host included — are its
    /// clients. Every communicator starts a new session, which streams
    /// nothing; what a peer offers stays with its provide route.
    pub fn rehost(&mut self, target: usize) {
        assert!(self.peers[target].online, "an offline peer hosts no room");
        self.host = target;
        for consumer in std::mem::take(&mut self.streams).into_keys() {
            self.deliver(consumer, StreamEndedMsg);
        }
        self.links.clear();
        let peer = self.peers[target].id;
        self.deliver(
            target,
            ConnectedMsg {
                is_host: true,
                peer,
            },
        );
        for index in 0..self.peers.len() {
            if index != target && self.peers[index].online {
                self.connect_client(index);
            }
        }
    }

    /// The peer's connections close; the host's communicator reports it,
    /// and the communicator of whoever streamed from it ends the stream.
    pub fn leave(&mut self, index: usize) {
        assert_ne!(index, self.host, "a host does not leave, see `rehost`");
        self.peers[index].online = false;
        self.peers[index].drain();
        self.links.remove(&index);
        let peer = self.peers[index].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer });
        self.end_streams_of(index);
    }

    /// The session of `index` ends: the stream it played ends with it, and
    /// so does every stream from it, as the provider's connections close.
    fn end_streams_of(&mut self, index: usize) {
        let ended: Vec<usize> = self
            .streams
            .iter()
            .filter(|(consumer, provider)| **consumer == index || **provider == index)
            .map(|(consumer, _)| *consumer)
            .collect();
        for consumer in ended {
            self.streams.remove(&consumer);
            if self.peers[consumer].online {
                self.deliver(consumer, StreamEndedMsg);
            }
        }
    }

    /// The streams of the offers withdrawn since the room last looked end:
    /// the provider's communicator turns their chunk requests down.
    fn withdrawn_offers(&mut self) -> Vec<(usize, IncomingMessage)> {
        let mut ended = Vec::new();
        for index in 0..self.peers.len() {
            let peer = &self.peers[index];
            let offer = peer.online.then(|| peer.model.video_provider.offered());
            let offer = offer.flatten();
            let before = match &offer {
                Some(video) => self.offers.insert(index, video.clone()),
                None => self.offers.remove(&index),
            };
            if before.is_none() || before == offer {
                continue;
            }
            let consumers: Vec<usize> = self
                .streams
                .iter()
                .filter(|(_, provider)| **provider == index)
                .map(|(consumer, _)| *consumer)
                .collect();
            for consumer in consumers {
                self.streams.remove(&consumer);
                ended.push((consumer, StreamEndedMsg.into()));
            }
        }
        ended
    }

    /// A ping to the host answered after `rtt`.
    pub fn round_trip(&mut self, index: usize, rtt: Duration) {
        self.deliver(index, HostRoundTripMsg { rtt });
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

    /// The next message the host sends `member` stays on its way until
    /// [`Room::release_link`]; what the host sends it after that waits
    /// behind it.
    pub fn hold_link(&mut self, member: usize) {
        let link = self.links.get_mut(&member).expect("not a member");
        link.held.get_or_insert_with(VecDeque::new);
    }

    /// The member answers the message on its way, and gets what waited
    /// behind it, in order.
    pub fn release_link(&mut self, member: usize) {
        let link = self.links.get_mut(&member).expect("not a member");
        let held = link.held.take().unwrap_or_default();
        for msg in held {
            let msg = self.links.get_mut(&member).expect("not a member").send(msg);
            if self.peers[member].online {
                self.deliver(member, msg);
            }
        }
    }

    /// The message on its way to `member` fails. The host drops the member,
    /// and with it what waited behind the message; the member's session
    /// ends with its connections, and it connects again.
    pub fn fail_link(&mut self, member: usize) {
        self.links.remove(&member);
        let peer = self.peers[member].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer });
        self.end_streams_of(member);
        self.connect_client(member);
    }

    /// The first message `client` sent since the last pump fails on its
    /// way to the host, which never handles it. The client's session ends
    /// as with a lost host, and what waited behind the message goes with
    /// it; the host sees the client's connections close, and the client
    /// connects again.
    pub fn fail_request(&mut self, client: usize) {
        assert_ne!(client, self.host, "the host sends itself nothing");
        self.peers[client].drain();
        self.links.remove(&client);
        let peer = self.peers[client].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer });
        self.end_streams_of(client);
        self.connect_client(client);
    }

    /// The network holds back everything `index` sent since the last pump:
    /// the test delivers it, late, with [`Room::release`].
    pub fn hold(&mut self, index: usize) -> Vec<OutgoingMessage> {
        self.peers[index].drain()
    }

    /// What the network held back of `from` arrives now.
    pub fn release(&mut self, from: usize, held: Vec<OutgoingMessage>) {
        for msg in held {
            for (to, msg) in self.route(from, msg) {
                if self.peers[to].online {
                    self.deliver(to, msg);
                }
            }
        }
    }

    /// A message reaches `to` the moment the test says so.
    pub fn deliver(&mut self, to: usize, msg: impl Into<IncomingMessage>) {
        let msg = msg.into();
        if let IncomingMessage::RoomState(state) = &msg {
            self.peers[to].states.push(state.clone());
        }
        msg.handle(&mut self.peers[to].model)
    }

    pub fn peer(&mut self, index: usize) -> &mut Peer {
        &mut self.peers[index]
    }

    pub fn host_index(&self) -> usize {
        self.host
    }

    /// Whether any peer online offers a file, so its communicator answers
    /// file requests.
    pub fn anyone_shares(&self) -> bool {
        let offers = |peer: &Peer| peer.online && peer.model.video_provider.offered().is_some();
        self.peers.iter().any(offers)
    }

    /// Who `consumer` streams from, as its communicator has it.
    pub fn provider_of(&self, consumer: usize) -> Option<usize> {
        self.streams.get(&consumer).copied()
    }

    /// Finish the reads in flight, then deliver messages until the room is
    /// quiescent. Panicking on too many rounds is the no-echo rule: no
    /// handler may re-emit the broadcast it is reacting to, or the room
    /// never settles.
    pub fn pump(&mut self) {
        for _ in 0..100 {
            let mut deliveries = self.withdrawn_offers();
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
    /// - the host sends the room state and the action lines to every member,
    ///   through the member's [`Link`]
    /// - a client's intent is a request to the host
    /// - a client's chat line goes to the host, which passes it on to every
    ///   member but its author; the host's goes to every member, so each
    ///   other peer gets each line once
    /// - `ToPeer` goes to exactly that peer, through its [`Link`]
    /// - a file request travels directly to the provider it names, whose
    ///   communicator answers it from what its provider offers
    fn route(&mut self, from: usize, msg: OutgoingMessage) -> Vec<(usize, IncomingMessage)> {
        // like the role handlers, a peer's messages of the other role's
        // group go nowhere
        let from_host = from == self.host;
        match msg {
            OutgoingMessage::Room(m) => self.route_room(from, m),
            OutgoingMessage::Host(m) if from_host => self.route_instruction(from, m),
            OutgoingMessage::Client(m) if !from_host => self.route_request(from, m),
            OutgoingMessage::Host(_) | OutgoingMessage::Client(_) => vec![],
        }
    }

    /// The host's instructions: to every member, or to one peer.
    fn route_instruction(
        &mut self,
        from: usize,
        msg: HostInstruction,
    ) -> Vec<(usize, IncomingMessage)> {
        let (members, msg): (Vec<usize>, IncomingMessage) = match msg {
            HostInstruction::RoomState(m) => (self.members(from), m.into()),
            HostInstruction::Action(m) => (self.members(from), m.into()),
            HostInstruction::ToPeer(m) => match self.index_of(&m.peer) {
                Some(to) => (vec![to], m.message.into()),
                None => (vec![], m.message.into()),
            },
        };
        members
            .into_iter()
            .filter_map(|to| self.send_to_member(to, msg.clone()))
            .collect()
    }

    /// Every peer online but the host `from`.
    fn members(&self, from: usize) -> Vec<usize> {
        (0..self.peers.len())
            .filter(|to| *to != from && self.peers[*to].online)
            .collect()
    }

    /// `msg` goes out to `member` now, or waits behind the message on its
    /// way there.
    fn send_to_member(
        &mut self,
        member: usize,
        msg: IncomingMessage,
    ) -> Option<(usize, IncomingMessage)> {
        let link = self.links.get_mut(&member)?;
        match &mut link.held {
            Some(queue) => {
                Link::wait(queue, msg);
                None
            }
            None => Some((member, link.send(msg))),
        }
    }

    /// A client's requests go directly to the host.
    fn route_request(
        &mut self,
        from: usize,
        request: ClientRequest,
    ) -> Vec<(usize, IncomingMessage)> {
        let peer = self.peers[from].id;
        let msg = match request {
            ClientRequest::Intent(intent) => PeerIntentMsg { peer, intent }.into(),
        };
        vec![(self.host, msg)]
    }

    fn route_room(&mut self, from: usize, msg: RoomMessage) -> Vec<(usize, IncomingMessage)> {
        let others = |m: IncomingMessage| -> Vec<(usize, IncomingMessage)> {
            (0..self.peers.len())
                .filter(|to| *to != from && self.peers[*to].online)
                .map(|to| (to, m.clone()))
                .collect()
        };
        match msg {
            RoomMessage::UserMessage(m) => others(m.into()),
            RoomMessage::Stream(m) => self.stream(from, m),
            RoomMessage::StreamEnded(_) => {
                self.streams.remove(&from);
                vec![]
            }
        }
    }

    /// The viewer's chunk requests go to the provider, whose communicator
    /// answers them from what its provider offers, without its core. The
    /// stream takes the place of the one the viewer played.
    fn stream(&mut self, from: usize, msg: StreamMsg) -> Vec<(usize, IncomingMessage)> {
        self.streams.remove(&from);
        let provider = self.index_of(&msg.provider).filter(|p| *p != from);
        let offered = provider.and_then(|p| self.peers[p].model.video_provider.offered());
        let wanted = self.peers[from].video_server.serves();
        match provider {
            Some(provider) if offered.is_some() && offered == wanted => {
                self.streams.insert(from, provider);
                vec![]
            }
            // the provider turns the chunk requests down or is gone: the
            // viewer's consumer shows why and gives up
            _ => vec![
                (from, NoticeMsg::new("Not providing any files").into()),
                (from, StreamEndedMsg.into()),
            ],
        }
    }
}
