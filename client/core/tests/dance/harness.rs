use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcstr::ArcStr;
use async_trait::async_trait;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::chunk::SharedFile;
use niketsu_core::communicator::{
    CommunicatorTrait, ConnectedMsg, DisconnectedMsg, EndpointInfo, HostRoundTripMsg,
    IncomingMessage, OutgoingMessage, PeerDisconnectedMsg, PeerId, PeerIntentMsg, RoomStateMsg,
};
use niketsu_core::config::Config;
use niketsu_core::file_database::{
    FileDatabaseEvent, FileDatabaseTrait, FileEntry, FileStore, UpdateComplete,
};
use niketsu_core::heartbeat::{HEARTBEAT_INTERVAL, Heartbeat};
use niketsu_core::player::{
    MediaPlayerEvent, MediaPlayerTrait, PlayerFailed, PlayerFileEnd, PlayerLoaded, Source,
};
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use niketsu_core::ui::{
    MessageSource, PlayerMessage, UserChange, UserInterfaceEvent, UserInterfaceTrait,
};
use niketsu_core::video_provider::{VideoProviderEvent, VideoProviderTrait};
use niketsu_core::{CoreModel, EventHandler, SavedPlaylistLoader};
use tokio::time::Instant;

// ------------------------------------------------------------------
// Fake player: stands in for mpv. It plays by the test's clock, which
// only moves when the test advances it. State is shared with the test via
// a handle so scenarios can simulate drift, slow seeks and slow loads.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PlayerState {
    /// What the player was told to play, and the video that is.
    pub source: Option<Source>,
    pub video: Option<Video>,
    /// Where the player is. While it has a loaded video and is not paused,
    /// it plays on from here as the test's clock advances.
    pub position: Duration,
    pub speed: f64,
    pub paused: bool,
    pub file_loaded: bool,
    /// When true, `load` leaves `file_loaded` false until
    /// [`PlayerHandle::finish_load`] — simulates mpv still opening the
    /// file.
    pub load_lag: bool,
    /// When true, `set_position` only notes the target and the position
    /// stays until [`PlayerHandle::land_seek`] — simulates mpv seeking.
    pub seek_lag: bool,
    seek_target: Option<Duration>,
    /// When the player was at `position`.
    at: Instant,
    /// What the player has to report to its core.
    events: Vec<MediaPlayerEvent>,
}

impl PlayerState {
    fn new() -> Self {
        Self {
            source: None,
            video: None,
            position: Duration::ZERO,
            speed: 1.0,
            paused: true,
            file_loaded: false,
            load_lag: false,
            seek_lag: false,
            seek_target: None,
            at: Instant::now(),
            events: Vec::new(),
        }
    }

    /// Plays on to the present.
    fn play_on(&mut self) {
        let now = Instant::now();
        if self.video.is_some() && self.file_loaded && !self.paused {
            self.position += (now - self.at).mul_f64(self.speed);
        }
        self.at = now;
    }
}

/// The video a source is of: the fake database keeps every file under its
/// name, a URL is its own video, and a stream is one of the file a peer
/// offers.
fn video_of(source: &Source) -> Video {
    match source {
        Source::File(path) => Video::from(path.file_name().unwrap().to_str().unwrap()),
        Source::Url(url) => Video::from(url.as_str()),
        Source::Stream { file, .. } => Video::from(file.name.as_str()),
    }
}

/// The player's state as it is at this moment.
fn present(state: &Mutex<PlayerState>) -> std::sync::MutexGuard<'_, PlayerState> {
    let mut state = state.lock().unwrap();
    state.play_on();
    state
}

#[derive(Debug, Clone)]
pub struct PlayerHandle(Arc<Mutex<PlayerState>>);

impl PlayerHandle {
    pub fn state(&self) -> PlayerState {
        present(&self.0).clone()
    }

    /// The player ends up in another state without telling its core, as a
    /// player does that drifts, or before it reports what its user did.
    pub fn set(&self, f: impl FnOnce(&mut PlayerState)) {
        f(&mut present(&self.0))
    }

    /// The player arrives where it was told to seek while `seek_lag` held
    /// it back.
    pub fn land_seek(&self) {
        let mut state = present(&self.0);
        let target = state.seek_target.take().expect("no seek in flight");
        state.position = target;
    }

    /// The player finishes the load `load_lag` held back, and reports it.
    pub fn finish_load(&self) {
        let mut state = present(&self.0);
        state.file_loaded = true;
        state.events.push(PlayerLoaded.into());
    }

    /// The peer and the file of the stream the player reads, if it reads
    /// one.
    pub fn stream(&self) -> Option<(PeerId, SharedFile)> {
        match self.0.lock().unwrap().source.clone() {
            Some(Source::Stream { provider, file }) => Some((provider, file)),
            _ => None,
        }
    }

    /// The chunks of the stream the player reads stop coming. Like mpv,
    /// it holds nothing from then on and reports that it failed.
    fn fail_stream(&self) {
        let mut state = present(&self.0);
        state.source = None;
        state.video = None;
        state.file_loaded = false;
        state.events.push(PlayerFailed.into());
    }

    fn take_events(&self) -> Vec<MediaPlayerEvent> {
        std::mem::take(&mut self.0.lock().unwrap().events)
    }
}

#[derive(Debug)]
struct FakePlayer(Arc<Mutex<PlayerState>>);

impl FakePlayer {
    fn new() -> (Self, PlayerHandle) {
        let state = Arc::new(Mutex::new(PlayerState::new()));
        (Self(state.clone()), PlayerHandle(state))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PlayerState> {
        present(&self.0)
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

    /// Like mpv, the player has no position while it seeks.
    fn get_position(&mut self) -> Option<Duration> {
        let state = self.lock();
        let landed = state.video.is_some() && state.seek_target.is_none();
        landed.then_some(state.position)
    }

    /// Like mpv, the player loads paused and reports when it has loaded.
    fn load(&mut self, source: Source, pos: Duration) {
        let mut state = self.lock();
        state.video = Some(video_of(&source));
        state.source = Some(source);
        state.position = pos;
        state.paused = true;
        state.seek_target = None;
        state.file_loaded = !state.load_lag;
        if state.file_loaded {
            state.events.push(PlayerLoaded.into());
        }
    }

    fn unload(&mut self) {
        let mut state = self.lock();
        state.video = None;
        state.source = None;
        state.file_loaded = false;
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

#[derive(Debug)]
struct FakeCommunicator {
    peer: PeerId,
    outbox: Outbox,
}

#[async_trait]
impl CommunicatorTrait for FakeCommunicator {
    fn peer(&self) -> PeerId {
        self.peer
    }

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
    /// The file at `path` is one the database knows from now on.
    fn add_path(&mut self, path: PathBuf) {
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        self.entries.push(FileEntry::new(name, path, None));
        self.store = FileStore::from_iter(self.entries.clone());
    }

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
        let id = peer_id(index);
        let communicator = FakeCommunicator {
            peer: id,
            outbox: Outbox::default(),
        };
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
            id,
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

    /// A core that is in no room, with nothing saved for any.
    pub fn in_no_room(name: &str, files: &[&str]) -> Self {
        Self::new(0, name, files, false, |_| None)
    }

    /// Perform a local action, e.g. a UI event or a player event.
    pub fn act(&mut self, event: impl EventHandler) {
        event.handle(&mut self.model)
    }

    pub fn heartbeat(&mut self) {
        Heartbeat.handle(&mut self.model)
    }

    /// The player reaches the end of its video, if it has one. Like mpv, it
    /// holds no video from then on.
    pub fn reach_end(&mut self) {
        if self.player.state().video.is_none() {
            return;
        }
        self.player.set(|player| {
            player.source = None;
            player.video = None;
            player.file_loaded = false;
        });
        self.act(PlayerFileEnd);
    }

    /// The core handles what its player has to report. Returns whether
    /// there was anything.
    pub fn hear_player(&mut self) -> bool {
        let events = self.player.take_events();
        let any = !events.is_empty();
        for event in events {
            event.handle(&mut self.model);
        }
        any
    }

    /// The room's playlist as this core has it.
    pub fn playlist(&self) -> Playlist {
        self.model.state.playlist.clone()
    }

    /// The peer's file database finds the file `name`.
    pub fn find_file(&mut self, name: &str) {
        self.model.database.add_path(format!("/{name}").into());
        self.act(UpdateComplete);
    }

    /// Whether this peer has `video` itself, to play it from.
    pub fn has(&self, video: &Video) -> bool {
        video.is_url() || self.model.database.find_file(video.as_str()).is_some()
    }

    /// The room's selected video as this core has it.
    pub fn selected(&self) -> Option<Video> {
        self.model.state.video.clone()
    }

    fn drain(&mut self) -> Vec<OutgoingMessage> {
        std::mem::take(&mut self.outbox.lock().unwrap())
    }
}

// ------------------------------------------------------------------
// Room: the in-memory network. Owns the routing rules of the dance.
// ------------------------------------------------------------------

/// What the host sends one member, as the member's stream in
/// `client/communicator/src/p2p/stream.rs` carries it: in order, one
/// message being written at a time, a waiting room state giving way to a
/// newer one, and each playlist once. Delivery is instant unless the test
/// holds the link.
#[derive(Debug, Default)]
struct Link {
    /// The last playlist written to the member.
    playlist: Option<Playlist>,
    /// While the test holds the link: the message being written, then
    /// what waits behind it.
    held: Option<VecDeque<IncomingMessage>>,
}

impl Link {
    /// `msg` as it goes out now: a room state without the playlist the
    /// member was sent already.
    fn send(&mut self, mut msg: IncomingMessage) -> IncomingMessage {
        if let IncomingMessage::RoomState(state) = &mut msg {
            if state.playlist == self.playlist {
                state.playlist = None;
            } else {
                self.playlist = state.playlist.clone();
            }
        }
        msg
    }

    /// Queues `msg` behind the one being written, where a newer room state
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
    /// The host's stream to each member, by the member's index.
    links: HashMap<usize, Link>,
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
            self.deliver(index, ConnectedMsg { is_host: true });
        } else {
            self.connect_client(index);
        }
        index
    }

    /// The host takes a client into the room: the host's communicator opens
    /// the member's link, the client's reports the connection. The host's
    /// core learns of the member from its status.
    fn connect_client(&mut self, index: usize) {
        self.links.insert(index, Link::default());
        self.deliver(index, ConnectedMsg { is_host: false });
    }

    fn index_of(&self, id: &PeerId) -> Option<usize> {
        self.peers.iter().position(|p| p.id == *id && p.online)
    }

    /// The host loses the room at the relay, and the room there closes
    /// with it. Everyone's session ends and everyone connects again;
    /// `target` is the first to arrive and hosts the room, the others —
    /// the old host included — are its clients. No chunk reaches anyone
    /// between the sessions, so every stream fails; what a peer offers
    /// stays with its provide route.
    pub fn rehost(&mut self, target: usize) {
        assert!(self.peers[target].online, "an offline peer hosts no room");
        self.host = target;
        self.fail_streams(|_, _| true);
        self.links.clear();
        for index in 0..self.peers.len() {
            if self.peers[index].online {
                self.end_session(index);
            }
        }
        self.deliver(target, ConnectedMsg { is_host: true });
        for index in 0..self.peers.len() {
            if index != target && self.peers[index].online {
                self.connect_client(index);
            }
        }
    }

    /// The peer's connections close; the host's communicator reports it,
    /// and the chunk requests of whoever streamed from it fail.
    pub fn leave(&mut self, index: usize) {
        assert_ne!(index, self.host, "a host does not leave, see `rehost`");
        self.peers[index].online = false;
        self.peers[index].drain();
        self.links.remove(&index);
        let peer = self.peers[index].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer });
        self.end_streams_of(index);
    }

    /// The session of `index` ends: the stream it played fails with it,
    /// and so does every stream from it, as the provider's connections
    /// close.
    fn end_streams_of(&mut self, index: usize) {
        let id = self.peers[index].id;
        self.fail_streams(|viewer, provider| viewer == index || *provider == id);
    }

    /// The player of every `viewer` online whose stream from `provider`
    /// has `ended` gets no more chunks, and its core hears of it with the
    /// next pump.
    fn fail_streams(&mut self, ended: impl Fn(usize, &PeerId) -> bool) {
        for (viewer, peer) in self.peers.iter().enumerate() {
            let Some((provider, _)) = peer.player.stream() else {
                continue;
            };
            if peer.online && ended(viewer, &provider) {
                peer.player.fail_stream();
            }
        }
    }

    /// The streams whose provider does not offer their file fail: the
    /// provider's communicator turns the chunk requests down, or the
    /// provider is gone.
    fn fail_streams_nobody_serves(&mut self) {
        let unserved: Vec<usize> = (0..self.peers.len())
            .filter(|viewer| {
                let Some((provider, file)) = self.peers[*viewer].player.stream() else {
                    return false;
                };
                let provider = self.index_of(&provider).filter(|p| p != viewer);
                let offered = provider.and_then(|p| self.peers[p].model.video_provider.offered());
                offered != Some(file)
            })
            .collect();
        self.fail_streams(|viewer, _| unserved.contains(&viewer));
    }

    /// A ping to the host answered after `rtt`.
    pub fn round_trip(&mut self, index: usize, rtt: Duration) {
        self.deliver(index, HostRoundTripMsg { rtt });
    }

    /// The clock moves on by `by`, and with it the room and every player
    /// that plays.
    pub async fn pass(&mut self, by: Duration) {
        tokio::time::advance(by).await;
    }

    /// The clock moves on by `by` with every pacemaker ticking, so each
    /// peer sees for that long how far its player is from the room.
    pub async fn watch(&mut self, by: Duration) {
        let ticks = by.as_millis() / HEARTBEAT_INTERVAL.as_millis();
        for _ in 0..ticks {
            self.pass(HEARTBEAT_INTERVAL).await;
            self.heartbeat();
            self.pump();
        }
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

    /// The next message the host sends `member` is not written through
    /// until [`Room::release_link`]; what the host sends it after that
    /// waits behind it.
    pub fn hold_link(&mut self, member: usize) {
        let link = self.links.get_mut(&member).expect("not a member");
        link.held.get_or_insert_with(VecDeque::new);
    }

    /// The member gets the message being written, and what waited behind
    /// it, in order.
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

    /// The communicator of `index` reports that its session ended.
    fn end_session(&mut self, index: usize) {
        let reason = "the session ended".to_string();
        self.deliver(index, DisconnectedMsg { reason });
    }

    /// The stream between `member` and the host ends. The host drops the
    /// member, and with it what waited to be written there; the member's
    /// session ends, and what it sent since the last pump goes with it.
    /// The member is in no room until [`Room::reconnect`].
    pub fn lose_connection(&mut self, member: usize) {
        assert_ne!(member, self.host, "a host loses its room, see `rehost`");
        self.peers[member].drain();
        self.links.remove(&member);
        let peer = self.peers[member].id;
        self.deliver(self.host, PeerDisconnectedMsg { peer });
        self.end_streams_of(member);
        self.end_session(member);
    }

    /// A member that lost its connection joins the room again.
    pub fn reconnect(&mut self, member: usize) {
        self.connect_client(member);
    }

    /// The stream between `member` and the host ends on a write that did
    /// not get through, whichever side wrote: what waited to be written to
    /// the member goes with it, and so does what the member sent since the
    /// last pump, which the host never handles. The member joins again.
    pub fn fail_link(&mut self, member: usize) {
        self.lose_connection(member);
        self.reconnect(member);
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

    /// Who the player of `viewer` reads a stream from.
    pub fn provider_of(&self, viewer: usize) -> Option<usize> {
        let (provider, _) = self.peers[viewer].player.stream()?;
        self.index_of(&provider)
    }

    /// Finish the reads in flight, then let every core hear its player and
    /// deliver messages until the room is quiescent. Panicking on too many
    /// rounds is the no-echo rule: no handler may re-emit the broadcast it
    /// is reacting to, or the room never settles.
    pub fn pump(&mut self) {
        for _ in 0..100 {
            self.fail_streams_nobody_serves();
            let mut heard = false;
            for peer in self.peers.iter_mut().filter(|peer| peer.online) {
                heard |= peer.hear_player();
            }
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
            if deliveries.is_empty() && !heard {
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
    /// - the host sends the room state and the lines to every member,
    ///   through the member's [`Link`], a chat line to every member but its
    ///   author
    /// - a client's intent is a request to the host
    ///
    /// The chunk requests of a player that reads a stream pass no core:
    /// they go directly to the provider, whose communicator answers them
    /// from what its provider offers, see
    /// [`Room::fail_streams_nobody_serves`].
    fn route(&mut self, from: usize, msg: OutgoingMessage) -> Vec<(usize, IncomingMessage)> {
        // like a role of the communicator, a peer sends only what its role
        // sends, and the rest goes nowhere
        let from_host = from == self.host;
        let peer = self.peers[from].id;
        match msg {
            OutgoingMessage::RoomState(m) if from_host => {
                self.send_to_members(from, None, m.into())
            }
            OutgoingMessage::Line(m) if from_host => {
                self.send_to_members(from, m.author(), m.into())
            }
            OutgoingMessage::Intent(intent) if !from_host => {
                vec![(self.host, PeerIntentMsg { peer, intent }.into())]
            }
            _ => vec![],
        }
    }

    /// What the host `from` sends, to every member but the `author` of a
    /// chat line.
    fn send_to_members(
        &mut self,
        from: usize,
        author: Option<PeerId>,
        msg: IncomingMessage,
    ) -> Vec<(usize, IncomingMessage)> {
        let members = self.members(from).into_iter();
        let members: Vec<usize> = members
            .filter(|to| Some(self.peers[*to].id) != author)
            .collect();
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
}
