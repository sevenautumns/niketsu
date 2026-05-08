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
