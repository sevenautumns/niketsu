use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcstr::ArcStr;
use async_trait::async_trait;
use libp2p::identity::Keypair;
use niketsu_communicator::P2PCommunicator;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::chunk::ChunkRoute;
use niketsu_core::config::Config;
use niketsu_core::file_database::{
    FileDatabaseEvent, FileDatabaseTrait, FileEntry, FilePathSearch, FileStore,
};
use niketsu_core::player::{
    MediaPlayerEvent, MediaPlayerTrait, PlayerPause, PlayerPositionChange, PlayerStart,
};
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use niketsu_core::ui::{
    PlayerMessage, UserChange, UserInterfaceEvent, UserInterfaceTrait, UserMessage,
};
use niketsu_core::user::UserStatus;
use niketsu_core::video_provider::VideoProvider;
use niketsu_video_server::VideoServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How long a scenario waits for the network to deliver. Generous, because
/// joining a room takes a relay round trip and a hole punch; a passing
/// scenario never waits this long.
const PATIENCE: Duration = Duration::from_secs(30);

/// Waits until `holds`, panicking with `what` if the network never gets
/// there.
pub async fn eventually(what: &str, holds: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !holds() {
        assert!(Instant::now() < deadline, "never happened: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Reads a whole video the way the player does: with a ranged HTTP request
/// to the local video server at `url`.
pub async fn download(url: &str) -> Vec<u8> {
    let addr = url.strip_prefix("http://").expect("not a video server url");
    let read = async {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nRange: bytes=0-\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        response
    };
    let mut response = tokio::time::timeout(PATIENCE, read)
        .await
        .expect("the video server never finished the response");
    let body = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("no HTTP header")
        + 4;
    response.split_off(body)
}

/// With `E2E_LOG` set, every peer of the scenario logs to stderr, which the
/// test runner shows for a failing scenario.
fn log_to_stderr() {
    if std::env::var_os("E2E_LOG").is_none() {
        return;
    }
    let filter = Targets::new()
        .with_target("niketsu", Level::DEBUG)
        .with_target("libp2p_swarm", Level::DEBUG);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter),
        )
        .try_init()
        .ok();
}

// ------------------------------------------------------------------
// Relay: the real relay server on a free loopback port.
// ------------------------------------------------------------------

pub struct Relay {
    port: u16,
    task: JoinHandle<()>,
}

impl Relay {
    pub fn start() -> Self {
        log_to_stderr();
        let keypair = Keypair::generate_ed25519();
        // The relay takes one port number for all its listeners, so ask the
        // system for a free one first. Free for one listener is not free for
        // all of them, and scenarios run in parallel, hence the retries.
        let (port, mut relay) = (0..10)
            .find_map(|_| {
                let port = UdpSocket::bind("127.0.0.1:0")
                    .ok()?
                    .local_addr()
                    .ok()?
                    .port();
                let relay = niketsu_relay::relay::Relay::new(keypair.clone(), port).ok()?;
                Some((port, relay))
            })
            .expect("relay failed to start");
        let task = tokio::spawn(async move { relay.run().await });
        Self { port, task }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ------------------------------------------------------------------
// Fake player: stands in for mpv. The test reads its state and plays the
// person at the keyboard through the handle.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub video: Option<Video>,
    pub position: Duration,
    pub paused: bool,
    /// Where the core told the player to read the video from once it became
    /// available, e.g. the URL of the local video server.
    pub source: Option<String>,
    speed: f64,
}

#[derive(Debug, Clone)]
pub struct PlayerHandle {
    state: Arc<Mutex<PlayerState>>,
    events: UnboundedSender<MediaPlayerEvent>,
}

impl PlayerHandle {
    pub fn state(&self) -> PlayerState {
        self.state.lock().unwrap().clone()
    }

    /// The person pauses in the player window.
    pub fn pause(&self) {
        self.state.lock().unwrap().paused = true;
        self.events.send(PlayerPause.into()).unwrap();
    }

    /// The person starts playback in the player window.
    pub fn start(&self) {
        self.state.lock().unwrap().paused = false;
        self.events.send(PlayerStart.into()).unwrap();
    }

    /// The player ends up at `position` on its own, as a player that plays
    /// does, without telling the core.
    pub fn drift(&self, position: Duration) {
        self.state.lock().unwrap().position = position;
    }

    /// The person seeks in the player window.
    pub fn seek(&self, position: Duration) {
        self.state.lock().unwrap().position = position;
        self.events
            .send(PlayerPositionChange::new(position).into())
            .unwrap();
    }
}

#[derive(Debug)]
struct FakePlayer {
    state: Arc<Mutex<PlayerState>>,
    events: UnboundedReceiver<MediaPlayerEvent>,
}

impl FakePlayer {
    fn new() -> (Self, PlayerHandle) {
        let state = Arc::new(Mutex::new(PlayerState {
            video: None,
            position: Duration::ZERO,
            paused: true,
            source: None,
            speed: 1.0,
        }));
        let (sender, events) = unbounded_channel();
        let handle = PlayerHandle {
            state: state.clone(),
            events: sender,
        };
        (Self { state, events }, handle)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PlayerState> {
        self.state.lock().unwrap()
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
        self.lock().position = pos;
    }

    fn get_position(&mut self) -> Option<Duration> {
        let state = self.lock();
        state.video.as_ref().map(|_| state.position)
    }

    fn load_video(&mut self, load: Video, pos: Duration, _db: &FileStore) {
        let mut state = self.lock();
        state.video = Some(load);
        state.position = pos;
    }

    fn unload_video(&mut self) {
        self.lock().video = None;
    }

    fn maybe_reload_video(&mut self, _f: &dyn FilePathSearch) {}

    fn reload_video(&mut self, f: &dyn FilePathSearch, filename: &str) {
        self.lock().source = f.get_file_path(filename);
    }

    fn playing_video(&self) -> Option<Video> {
        self.lock().video.clone()
    }

    fn video_loaded(&self) -> bool {
        self.lock().video.is_some()
    }

    async fn event(&mut self) -> MediaPlayerEvent {
        match self.events.recv().await {
            Some(event) => event,
            None => std::future::pending().await,
        }
    }
}

// ------------------------------------------------------------------
// Fake UI: records what the core shows and passes on what the test
// clicks.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct UiState {
    pub playlist: Playlist,
    pub playing_video: Option<Video>,
    /// `None` until the core has reported a role, i.e. before the first
    /// connection.
    pub is_host: Option<bool>,
    pub video_share: bool,
    pub users: UserList,
    /// The lines of the chat, oldest first.
    pub messages: Vec<String>,
}

impl UiState {
    /// The names in the user list, sorted.
    pub fn user_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.users.iter().map(|u| u.name.to_string()).collect();
        names.sort();
        names
    }

    /// The names of the users shown offering the selected video, sorted.
    pub fn offering(&self) -> Vec<String> {
        let offers = |u: &&UserStatus| {
            let offered = u.offers.as_ref().map(|f| f.name.as_str());
            offered.is_some() && offered == self.playing_video.as_ref().map(|v| v.as_str())
        };
        let mut names: Vec<_> = self
            .users
            .iter()
            .filter(offers)
            .map(|u| u.name.to_string())
            .collect();
        names.sort();
        names
    }
}

#[derive(Debug, Clone)]
pub struct UiHandle {
    state: Arc<Mutex<UiState>>,
    events: UnboundedSender<UserInterfaceEvent>,
}

impl UiHandle {
    pub fn state(&self) -> UiState {
        self.state.lock().unwrap().clone()
    }

    /// The person does something in the UI.
    pub fn act(&self, event: impl Into<UserInterfaceEvent>) {
        self.events.send(event.into()).unwrap();
    }

    /// The person sends a chat line, which the UI shows right away, as
    /// `UserInterface::send_message` does.
    pub fn say(&self, message: &str) {
        self.state
            .lock()
            .unwrap()
            .messages
            .push(message.to_string());
        self.act(UserMessage {
            message: message.to_string(),
        });
    }
}

#[derive(Debug)]
struct FakeUi {
    state: Arc<Mutex<UiState>>,
    events: UnboundedReceiver<UserInterfaceEvent>,
}

impl FakeUi {
    fn new() -> (Self, UiHandle) {
        let state = Arc::new(Mutex::new(UiState::default()));
        let (sender, events) = unbounded_channel();
        let handle = UiHandle {
            state: state.clone(),
            events: sender,
        };
        (Self { state, events }, handle)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UiState> {
        self.state.lock().unwrap()
    }
}

#[async_trait]
impl UserInterfaceTrait for FakeUi {
    fn file_database_status(&mut self, _update_status: f32) {}
    fn file_database(&mut self, _db: FileStore) {}

    fn playlist(&mut self, playlist: Playlist) {
        self.lock().playlist = playlist;
    }

    fn video_change(&mut self, video: Option<Video>) {
        self.lock().playing_video = video;
    }

    fn user_list(&mut self, user_list: UserList) {
        self.lock().users = user_list;
    }

    fn user_update(&mut self, _user: UserChange) {}

    fn player_message(&mut self, msg: PlayerMessage) {
        self.lock().messages.push(msg.message.clone());
    }

    fn username_change(&mut self, _username: ArcStr) {}
    fn abort(&mut self) {}

    fn video_share(&mut self, video_share: bool) {
        self.lock().video_share = video_share;
    }

    fn is_host(&mut self, is_host: bool) {
        self.lock().is_host = Some(is_host);
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        match self.events.recv().await {
            Some(event) => event,
            None => std::future::pending().await,
        }
    }
}

// ------------------------------------------------------------------
// Fake file database: every peer has its files, nothing is scanned.
// ------------------------------------------------------------------

#[derive(Debug)]
struct FakeDatabase {
    entries: Vec<FileEntry>,
    store: FileStore,
}

impl FakeDatabase {
    fn new(entries: Vec<FileEntry>) -> Self {
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
// Peer: one running core and the handles of the person using it. The
// video server and provider are the real ones, wired to the communicator
// like in the client binary.
// ------------------------------------------------------------------

pub struct Peer {
    pub player: PlayerHandle,
    pub ui: UiHandle,
    name: String,
    core: JoinHandle<()>,
}

impl Peer {
    /// Starts a core that connects to the scenario's room through `relay`.
    /// The first to arrive becomes the host, see [`Peer::hosting`]. The
    /// peer knows `files` by name only; nothing is on disk.
    pub fn join(relay: &Relay, name: &str, files: &[&str]) -> Self {
        let files = files
            .iter()
            .map(|name| FileEntry::new(name.to_string(), format!("/{name}").into(), None))
            .collect();
        Self::join_with_files(relay, name, files)
    }

    /// Like [`Peer::join`], for a peer whose files exist on disk.
    pub fn join_with_files(relay: &Relay, name: &str, files: Vec<FileEntry>) -> Self {
        Self::start(relay, name, files, "secret")
    }

    /// Like [`Peer::join`], for a peer that knows the room by `password`.
    pub fn join_with_password(relay: &Relay, name: &str, password: &str) -> Self {
        Self::start(relay, name, vec![], password)
    }

    fn start(relay: &Relay, name: &str, files: Vec<FileEntry>, password: &str) -> Self {
        let fetch = ChunkRoute::default();
        let provide = ChunkRoute::default();
        let communicator = P2PCommunicator::new(fetch.clone(), provide.clone()).without_mdns();
        let (player, player_handle) = FakePlayer::new();
        let (ui, ui_handle) = FakeUi::new();
        let config = Config {
            username: ArcStr::from(name),
            relay: "127.0.0.1".to_string(),
            port: relay.port,
            room: ArcStr::from("e2e"),
            password: password.to_string(),
            auto_connect: true,
            ..Default::default()
        };
        let core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(FakeDatabase::new(files)))
            .video_server(Box::new(VideoServer::new(fetch)))
            .video_provider(Box::new(VideoProvider::new(provide)))
            .config(config)
            .saved_playlist(|_| None)
            .save_playlist(|_, _| {})
            .build();
        Self {
            player: player_handle,
            ui: ui_handle,
            name: name.to_string(),
            core: tokio::spawn(core.run()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Waits until the relay has made this peer the host, which it does for
    /// the first peer to arrive.
    pub async fn hosting(self) -> Self {
        eventually(&format!("{} hosts the room", self.name), || {
            self.ui.state().is_host == Some(true)
        })
        .await;
        self
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.core.abort();
    }
}
