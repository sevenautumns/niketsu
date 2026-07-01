# CLAUDE.md

Notes for working in this repo. Only things that aren't already obvious from `ls`, `grep`, `cargo metadata`, or the workspace `Cargo.toml`.

## What this is

niketsu — mpv-based video sync between peers over libp2p. Cargo workspace: client binary, relay-server binary, mdbook docs.

## Build / dev environment — Nix

**Everything goes through Nix.** Direnv (`.envrc` → `use flake`) drops you into the dev shell automatically; if direnv isn't active, run `nix develop`. The shell provides the rust toolchain (fenix stable + latest rustfmt), mpv, treefmt, nixfmt, cargo-nextest, cargo-watch, mdbook, yt-dlp, and the Windows cross-toolchain.

**Do not suggest `rustup`, `brew install mpv`, `apt install ...`, or any host-level installs.** If a tool is missing, the answer is to add it to the flake's `devShells.default` or to enter the shell.

Common commands:

```bash
cargo build --release                      # everything
cargo build --release --bin niketsu        # client only
cargo build --release --bin niketsu-relay
cargo nextest run                          # tests (CI uses nextest)
treefmt                                    # rustfmt + nixfmt across the tree — use this instead of `cargo fmt`
nix build .#niketsu-client
nix build .#niketsu-relay
nix flake check                            # what CI runs (clippy lives here)
```

Runtime: the client requires `mpv` to be reachable, which the nix shell provides on macOS.

## Version control — jj *and* git

This is a **jj-colocated** repo: `.jj` and `.git` exist side by side. Either tool works; pick whichever fits the task.

- Run `jj st` or `git status` before assuming a state — the user toggles between them.
- jj is usually nicer for local history rewrites; git is needed for `gh` (PRs, CI checks). `main` is the trunk on both sides.
- The branch shown by `git status` may read `HEAD` (detached) when jj is driving — that's not a bug, it's the colocation. Use `jj log -r 'present(@)|trunk()'` for the jj-side view.

## Workspace layout

```
client/                       niketsu (binary; wires everything together; entry: src/main.rs)
client/core/                  niketsu-core: traits, CoreModel, the event loop
client/communicator/          niketsu-communicator: libp2p (gossipsub, relay, QUIC/TCP)
client/player/mpv/            niketsu-mpv: libmpv FFI
client/player/video_server/   niketsu-video-server: HTTP server that feeds mpv from peer chunks
                              (path dep from client/, NOT in workspace `members`)
client/ui/iced/               niketsu-iced (feature-gated GUI)
client/ui/ratatui/            niketsu-ratatui (feature-gated TUI)
relay/                        niketsu-relay (libp2p relay + room/auth server)
book/                         mdbook user docs
```

Workspace `Cargo.toml` pins shared dep versions — check there before assuming a crate's version.

## Architecture — the non-obvious bits

- **Event-loop dispatch via `EventHandler`.** `Core::run_loop` in `client/core/src/lib.rs` is a `tokio::select!` across every subsystem (communicator, player, UI, video_server, video_provider, file_database, heartbeat). Each event type implements `EventHandler::handle(&mut CoreModel)` and mutates state + fans out side effects. To add an event: define the struct, impl `EventHandler`, wire it into the matching enum (usually via `enum_dispatch`). Don't create side channels — go through the loop.
- **Subsystems behind traits in `niketsu-core`.** `CommunicatorTrait`, `MediaPlayerTrait`, `UserInterfaceTrait`, `FileDatabaseTrait`, `VideoServerTrait`, `VideoProviderTrait`. All are `#[cfg_attr(test, mockall::automock)]`; core tests use the generated `Mock*Trait` types.
- **`Observed<T>` is the reactive primitive** (`client/core/src/util/observed.rs`): `ArcSwap<T>` + `Notify`. Core writes via `set()`; UIs `await changed()` then read via `get_inner()`. Reach for this before adding ad-hoc channels or `RwLock` for UI-visible state.
- **UI → core is mpsc-only.** UIs emit `UserInterfaceEvent`s through an unbounded channel that `run_loop` drains. The UI never touches `CoreModel` directly.
- **macOS threading.**
  - Ratatui: mpv needs `NSApplication` on the main thread, so app logic runs on a background thread and main runs the Cocoa loop. See `client/src/main.rs`.
  - Iced: iced/winit already owns the main thread; mpv plays inside that.
- **Iced is on 0.14.** Overlay layout calls `as_widget_mut()` (not `as_widget()`). Older snippets / model suggestions targeting 0.13 won't compile cleanly — adapt.

## Code style

- `rustfmt.toml`: `group_imports = "StdExternalCrate"`, `imports_granularity = "Module"`. Use `treefmt`, not `cargo fmt`, so `.nix` files get formatted too.
- Edition 2024 across the workspace.
- `enum_dispatch` is used heavily on hot dispatch paths — when adding a message or event, follow the surrounding pattern instead of switching to `dyn Trait`.

## Runtime config

`Config::load_or_default()` (`client/core/src/config.rs`) reads `~/.config/niketsu/config.toml` (or platform equivalent). Fields: `username`, `media_dirs`, `relay`, `port`, `room`, `password`, `auto_connect`, `auto_share`. Default relay is `autumnal.de:7766`.
