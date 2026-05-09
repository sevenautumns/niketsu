# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is niketsu

Video synchronization between multiple clients using mpv and libp2p peer-to-peer networking. Consists of a client binary and a relay server binary.

## Commands

```bash
# Build everything
cargo build --release

# Build specific binary
cargo build --release --bin niketsu
cargo build --release --bin niketsu-relay

# Run tests (CI uses nextest)
cargo nextest run

# Run a single test
cargo nextest run <test_name>
cargo test <test_name>  # fallback if nextest unavailable

# Format (runs rustfmt + nixfmt)
treefmt

# Nix dev shell (provides all build dependencies incl. mpv)
nix develop

# Nix builds
nix build .#niketsu-client
nix build .#niketsu-relay

# CI checks
nix flake check
```

Requires `mpv` installed when running the client. On macOS, `nix develop` is the easiest way to get all dependencies.

## Workspace layout

```
client/
  src/           # niketsu binary (entry point, wires all components together)
  core/          # niketsu-core: traits, event types, CoreModel, Core loop
  communicator/  # niketsu-communicator: libp2p P2P networking
  player/
    mpv/         # niketsu-mpv: FFI bindings to libmpv
    video_server/ # niketsu-video-server: TCP server for P2P video file sharing
  ui/
    iced/        # niketsu-iced: Iced GUI frontend (optional feature)
    ratatui/     # niketsu-ratatui: Ratatui TUI frontend (optional feature)
relay/           # niketsu-relay binary: libp2p relay/rendezvous server
```

## Architecture

### Core event loop

`Core::run_loop` in `client/core/src/lib.rs` is a `tokio::select!` loop that dispatches events from all subsystems to `CoreModel` via the `EventHandler` trait:

```
communicator (network) ─┐
player (mpv)            ├─→ EventHandler::handle(&mut CoreModel)
ui (user actions)       │
video_server            │
video_provider          │
pacemaker (heartbeat)   ┘
file_database
```

Every incoming event type implements `EventHandler`, which mutates `CoreModel` and may call out to other subsystems (e.g. send a network message, update the UI, load a video).

### Traits

All subsystems are behind traits defined in `niketsu-core`:
- `CommunicatorTrait` — connect to relay, send/receive network messages
- `MediaPlayerTrait` — control mpv playback
- `UserInterfaceTrait` — push state updates to the UI, receive UI events
- `FileDatabaseTrait` — index local media files, fuzzy search
- `VideoServerTrait` — serve video chunks over TCP
- `VideoProviderTrait` — request/receive video chunks from peers

Traits are mockable via `mockall` (`#[cfg_attr(test, mockall::automock)]`). Tests in `niketsu-core` use `MockXxxTrait` types directly.

### UI backends

Both `niketsu-iced` and `niketsu-ratatui` implement `UserInterfaceTrait`. The UI crate receives a `UiModel` (from `core/src/ui.rs`) which holds `Observed<T>` values that the UI polls for changes via a shared `Notify`.

`UiModel` methods (e.g. `change_video`, `send_message`) send `UserInterfaceEvent` values back to the core loop through an unbounded mpsc channel.

### Observed<T>

`Observed<T>` (`client/core/src/util/observed.rs`) is the reactive state primitive: an `ArcSwap<T>` paired with a `Notify`. UI code calls `observed.changed()` to check for updates and `observed.get_inner()` to read the value. The core sets values via `observed.set(v)` which atomically stores and wakes the notify.

### P2P networking

`niketsu-communicator` uses libp2p (gossipsub, relay, QUIC/TCP). The relay server (`niketsu-relay`) is a libp2p relay + request-response server for room join/auth. Clients connect to the relay at `autumnal.de:7766` by default (configurable in `config.toml`).

### macOS threading

On macOS with the ratatui UI, the app logic runs on a background thread and the main thread runs `NSApplication` (needed by mpv for its video window). With the iced UI, iced/winit owns the main thread. This is handled in `client/src/main.rs`.

## Configuration

Config is stored at the platform config dir (e.g. `~/.config/niketsu/config.toml` on Linux). Loaded via `Config::load_or_default()`. Fields: `username`, `media_dirs`, `relay`, `port`, `room`, `password`, `auto_connect`, `auto_share`.

## Code style

- `rustfmt.toml`: `group_imports = "StdExternalCrate"`, `imports_granularity = "Module"`
- Edition 2024 throughout
- `enum_dispatch` is used heavily to avoid dynamic dispatch on hot event paths

## Module reference

### niketsu-core (client/core/src/)

**lib.rs**
- `CoreModel` (line 44): central state — holds all subsystem boxes + `playlist: PlaylistHandler`, `ready`, `running`
- `Core` (line 60): event loop — `run()`, `auto_connect()`, `run_loop()` (tokio::select! over all subsystems)
- `EventHandler` trait (line 40): `handle(&mut CoreModel)` — every event type implements this

**communicator.rs**
- `CommunicatorTrait` (line 25): `connect(EndpointInfo)`, `send(OutgoingMessage)`, `receive()`, `has_endpoint()`
- `OutgoingMessage` enum (line 40): VideoStatus, Start, Pause, PlaybackSpeed, Seek, Select, UserMessage, Playlist, UserStatus, FileRequest/Response, ChunkRequest/Response, VideoShareChange
- `IncomingMessage` enum (line 59): all of OutgoingMessage + Connected, ConnectionError, UserStatusList, ServerMessage, VideoProviderStopped — dispatched via enum_dispatch
- All `*Msg` structs implement `EventHandler` in the same file

**player/mod.rs**
- `MediaPlayerTrait` (line 20): start/pause, set_speed, set/get_position, load_video/unload_video, cache_available, playing_video, async event()
- `PlayerPause/Start/CachePause/PositionChange/SpeedChange/FileEnd/Exit` — all implement EventHandler

**player/wrapper.rs**
- `MediaPlayerWrapper` (line 24): wraps any `MediaPlayerTrait`; `reconcile(pos)` (line 40) does flexible clock sync

**playlist/mod.rs**
- `Video` (line 16): Arc-wrapped; `From<&str>` parses file vs URL; implements `FuzzyEntry`
- `VideoInner` enum (line 49): `File(ArcStr)` | `Url(Arc<Url>)` — `is_url()`, `to_path_str()`, `as_str()`
- `Playlist` (line 114): Vec-backed; navigation (`iter`, `find`, `get_range`) + mutation (`push`, `insert`, `remove`, `move_video`, `move_range`); implements `FuzzySearchable<Video>`

**playlist/handler.rs**
- `PlaylistHandler` (line 6): Playlist + `playing: Option<usize>` — `get_current_video()`, `advance_to_next()`, `select_playing()`, `unload_playing()`, `replace()`

**file_database/mod.rs**
- `FileDatabaseTrait` (line 30): `add/del/clear_paths()`, `start/stop_update()`, `find_file()`, `all_files()`, `async event()`
- `FileEntry` (line 88) / `FileEntryInner` (line 126): Arc-wrapped file record — `path: PathBuf`, `name: ArcStr`, `modified`
- `FileDatabase` (line 182): holds `update: Option<JoinHandle<...>>`, `store: FileStore`, `paths: BTreeSet<PathBuf>`
- `FileStore` (line 324): immutable im::Vector — `find_file()` binary search; implements `FuzzySearchable<FileEntry>`
- `FilePathSearch` trait (line 334): `get_file_path(filename) -> Option<String>` — impls: `FileStore`, `VideoServerFile`

**ui.rs**
- `UserInterfaceTrait` (line 30): notifications (`file_database_status`, `player_message`, `user_list`, `video_change`), config methods, `async event()`, `abort()`
- UI events implement EventHandler: `PlaylistChange:60`, `VideoChange:78`, `RoomChange:128`, `UserChange:155`, `UserMessage:172`, `FileDatabaseChange`, `FileShareChange`, `SettingsChange`, `FileRequest`
- `PlayerMessage` (line 188): Arc-wrapped timestamped chat/status message; `MessageSource` and `MessageLevel` enums

**config.rs**
- `Config` (line 16): `username`, `media_dirs`, `relay`, `port`, `room`, `password`, `auto_connect`, `auto_share`; `addr() -> Multiaddr`, `load_or_default()`

**user.rs** — `UserStatus` (line 8): `name: ArcStr`, `ready: bool`; implements Ord for BTreeSet

**room.rs** — `RoomName = ArcStr`; `UserList` (line 11): BTreeSet-wrapped with `room: RoomName`

**video_provider.rs**
- `VideoProviderTrait` (line 16): `start/stop_providing()`, `request_chunk(uuid, filename, start, len)`, `size()`, `sharing()`, `async event()`

**video_server.rs**
- `VideoServerTrait` (line 15): `start_server(filename, size)`, `stop_server()`, `insert_chunk(filename, start, bytes)`, `addr()`, `async event()`
- `VideoServerFile` (line 66): implements `FilePathSearch` — returns HTTP URL for mpv

**fuzzy.rs**
- `FuzzyEntry` trait (line 16): `key() -> &str`
- `FuzzySearchable<E>` (line 20): `fuzzy_search(query) -> FuzzySearch<E>`, `len()`, `is_empty()`
- `FuzzySearch<E>` (line 33): rayon-backed `Future<Output=Vec<FuzzyResult<E>>>` — abortable

**heartbeat.rs** — `Pacemaker` (line 12): emits `Heartbeat` every 500ms; `Heartbeat` EventHandler sends `VideoStatusMsg`

**util/observed.rs** — `Observed<T>`: ArcSwap<T> + Notify — `set(v)`, `get_inner()`, `changed()`

### niketsu-communicator (client/communicator/src/)

**lib.rs**
- `Connection` enum (line 22): `Connected(Connected)` | `Connecting(Connecting)` | `Disconnected(Disconnected)` — state machine driven by `receive()`
- `P2PCommunicator` (line 160): implements `CommunicatorTrait`
- `Disconnected` (line 136): exponential backoff reconnect

**messages.rs** — `NiketsuMessage` (line 8): JSON transport envelope; `JoinMessage` (line 76): initial auth

**p2p/mod.rs**
- `Behaviour` (line 38): libp2p composition — relay_client, gossipsub, fileshare_request_response, message_request_response, init_request_response, kademlia, mdns
- `InitRequest/InitResponse` (lines 73-91): room auth with SHA256 password hash
- `CommunicationHandlerTrait` (line 94): `async run()`, handle_swarm_event/core_message/swarm_request/response/broadcast
- `Handler` enum (line 118): dispatches between `Client` and `Host` roles

**p2p/client.rs** — client (peer) role; **p2p/host.rs** — host (room owner) role; **p2p/file_share.rs** — `FileShareRequest/ResponseResult`

### niketsu-mpv (client/player/mpv/src/)

**lib.rs**
- `MpvProperty` enum (line 33): PlaybackTime, Pause, Speed, Filename, Duration, CachePause, EofReached, etc. — `format() -> mpv_format`
- `MpvCommand` enum (line 89): Loadfile, Stop, ShowText
- `MpvStatus` (line 109): player state cache — paused, seeking, speed, file, file_load_status, load_position
- `Mpv` (line 141): main wrapper; implements `MediaPlayerTrait` + `Drop` (unsafe terminate_destroy)

**bindings.rs** — raw FFI to libmpv C API; **event.rs** — `MpvEventPipe`, `PropertyValue` enum

### niketsu-video-server (client/player/video_server/src/)

**lib.rs**
- `CHUNK_SIZE = 512_000` (512KB), `TIMEOUT = 2s`, `MAX_RETRY = 3`
- `VideoServer` (line 27): implements `VideoServerTrait`
- `VideoCache` (line 68): moka LRU cache + notify — `obtain_chunk()` with retry
- `TcpServer` (line 177): HTTP range-request handler for mpv streaming

### niketsu-iced (client/ui/iced/src/)

- Currently on **iced 0.14** (note: overlay layout uses `as_widget_mut()`, not `as_widget()`)
- `TEXT_SIZE: ArcSwap<f32>` in lib.rs: dynamic text size
- `view.rs` — `IcedUI` implementing `UserInterfaceTrait`
- `styling.rs` — `FileButton`, `FileRuleTheme` widget styles
- `widget/`: chat, database, file_search (+ message.rs), overlay (`ElementOverlay`/`ElementOverlayConfig`), playlist, rooms, settings

### niketsu-ratatui (client/ui/ratatui/src/)

- `view` — `RatatuiUI` implementing `UserInterfaceTrait`
- `handler/`: chat_input, chat, command, help, login, media, options, playlist_browser, playlist, recently, search, settings, users

### niketsu-relay (relay/src/)

**relay.rs**
- `Relay` (line 63): `swarm`, `rooms: Arc<RwLock<HashMap<RoomName,(PeerId,PasswordHash)>>>`, `hosts: Arc<RwLock<HashMap<PeerId,RoomName>>>`
- `InitRequest/InitResponse`: bcrypt password verification
- `new(config)` (line 69): QUIC + TCP + Noise + Yamux; listens on all interfaces

### Client entry point (client/src/main.rs)

- `run_app(args)` (line 19): wires Mpv + P2PCommunicator + VideoServer + VideoProvider + FileDatabase → CoreBuilder → Core
- **Iced**: UI runs on main thread; **Ratatui on macOS**: core on background thread, NSApplication on main

## Cross-crate trait implementations

| Trait | Implementation crate |
|---|---|
| `CommunicatorTrait` | `P2PCommunicator` (niketsu-communicator) |
| `MediaPlayerTrait` | `Mpv` (niketsu-mpv) via `MediaPlayerWrapper` |
| `UserInterfaceTrait` | `IcedUI` (niketsu-iced), `RatatuiUI` (niketsu-ratatui) |
| `FileDatabaseTrait` | `FileDatabase` (niketsu-core) |
| `VideoServerTrait` | `VideoServer` (niketsu-video-server) |
| `VideoProviderTrait` | `VideoProvider` (niketsu-core) |
| `FilePathSearch` | `FileStore` (direct), `VideoServerFile` (HTTP URL) |

## Video chunk streaming flow

1. UI selects video → `VideoChange` → core calls `video_provider.start_providing(file)`
2. Peer sends `FileRequest` → EventHandler responds with file size
3. `VideoServer.start_server(filename, size)` starts HTTP server; mpv reads via `VideoServerFile` URL
4. mpv range requests → `ChunkRequest` events → sent to peer via communicator
5. Peer `ChunkResponse` → `VideoServer.insert_chunk()` → unblocks mpv read
