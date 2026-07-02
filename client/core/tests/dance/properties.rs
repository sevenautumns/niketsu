//! Property tests: the dance fuzzed.
//!
//! Scenarios pin sequences someone thought of; sync bugs live in the
//! orderings nobody chose. proptest generates random sequences of user
//! actions, the harness executes them deterministically, and afterwards
//! the room must satisfy the invariants below. On failure, proptest
//! shrinks to the minimal action sequence that still breaks them.

use std::time::Duration;

use niketsu_core::player::{PlayerFileEnd, PlayerPause, PlayerPositionChange, PlayerStart};
use niketsu_core::playlist::Playlist;
use niketsu_core::{ui, video_provider};
use proptest::prelude::*;

use crate::harness::Room;

const POOL: &[&str] = &["ep1", "ep2", "ep3"];

/// Everything a user (or their mpv) can do to the room. Peer indices are
/// taken modulo the current peer count when applied.
#[derive(Debug, Clone)]
enum Action {
    ChangePlaylist { peer: usize, videos: Vec<usize> },
    Select { peer: usize, video: usize },
    Seek { peer: usize, secs: u64 },
    Pause { peer: usize },
    Start { peer: usize },
    ReadyToggle { peer: usize },
    ShareToggle { peer: usize },
    FileRequest { peer: usize },
    FileEnd { peer: usize },
    HostHeartbeat,
    Handover { target: usize },
    Join,
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (0..8usize, prop::collection::vec(0..POOL.len(), 0..4))
            .prop_map(|(peer, videos)| Action::ChangePlaylist { peer, videos }),
        (0..8usize, 0..POOL.len()).prop_map(|(peer, video)| Action::Select { peer, video }),
        (0..8usize, 0..500u64).prop_map(|(peer, secs)| Action::Seek { peer, secs }),
        (0..8usize).prop_map(|peer| Action::Pause { peer }),
        (0..8usize).prop_map(|peer| Action::Start { peer }),
        (0..8usize).prop_map(|peer| Action::ReadyToggle { peer }),
        (0..8usize).prop_map(|peer| Action::ShareToggle { peer }),
        (0..8usize).prop_map(|peer| Action::FileRequest { peer }),
        (0..8usize).prop_map(|peer| Action::FileEnd { peer }),
        Just(Action::HostHeartbeat),
        (0..8usize).prop_map(|target| Action::Handover { target }),
        Just(Action::Join),
    ]
}

fn apply(room: &mut Room, action: Action) {
    match action {
        Action::ChangePlaylist { peer, videos } => {
            let peer = peer % room.peers.len();
            let playlist = Playlist::from_iter(videos.into_iter().map(|v| POOL[v]));
            // the real UI shows its own edit immediately
            // (UiModel::change_playlist); the core only echoes remote ones
            room.peer(peer).ui.set(|ui| ui.playlist = playlist.clone());
            room.peer(peer).act(ui::PlaylistChange { playlist });
        }
        Action::Select { peer, video } => {
            let peer = peer % room.peers.len();
            room.peer(peer).act(ui::VideoChange {
                video: POOL[video].into(),
            });
        }
        Action::Seek { peer, secs } => {
            // mpv already jumped before the event reaches the core
            let peer = peer % room.peers.len();
            let pos = Duration::from_secs(secs);
            room.peer(peer).player.set(|player| player.position = pos);
            room.peer(peer).act(PlayerPositionChange::new(pos));
        }
        Action::Pause { peer } => {
            let peer = peer % room.peers.len();
            room.peer(peer).player.set(|player| player.paused = true);
            room.peer(peer).act(PlayerPause);
        }
        Action::Start { peer } => {
            let peer = peer % room.peers.len();
            room.peer(peer).player.set(|player| player.paused = false);
            room.peer(peer).act(PlayerStart);
        }
        Action::ReadyToggle { peer } => {
            let peer = peer % room.peers.len();
            let name = room.peer(peer).name();
            let ready = !room.peer(peer).model.ready;
            room.peer(peer).act(ui::UserChange { name, ready });
        }
        Action::ShareToggle { peer } => {
            let peer = peer % room.peers.len();
            room.peer(peer).act(ui::FileShareChange {});
            // if the toggle started providing, the provider task reports
            // the file ready, which announces the share to the room
            if let Some(file_name) = room.peer(peer).model.video_provider.file_name() {
                room.peer(peer).act(video_provider::FileReady {
                    file_name,
                    size: 1_000,
                });
            }
        }
        Action::FileRequest { peer } => {
            // a no-op unless the peer plays a video it does not have on
            // disk (see ui::FileRequest)
            let peer = peer % room.peers.len();
            room.peer(peer).act(ui::FileRequest {});
        }
        Action::FileEnd { peer } => {
            let peer = peer % room.peers.len();
            if let Some(video) = room.peer(peer).player.state().video {
                room.peer(peer).act(PlayerFileEnd(video));
            }
        }
        Action::HostHeartbeat => {
            let host = room.host_index();
            room.peer(host).heartbeat();
        }
        Action::Handover { target } => {
            let target = target % room.peers.len();
            let host = room.host_index();
            if target != host {
                let new_host = room.peer(target).name();
                room.peer(host).act(ui::HostHandover { new_host });
            }
        }
        Action::Join => {
            if room.peers.len() < 4 {
                let name = format!("peer{}", room.peers.len());
                // late joiners bring no local files, so they exercise the
                // file-request path
                room.join(&name, &[]);
            }
        }
    }
    room.pump();
}

/// Every peer agrees on the playlist, the playlist marker, the loaded
/// video, and what its UI shows.
fn assert_room_converged(room: &mut Room) {
    let playlist = room.peer(0).playlist();
    let marker = room.peer(0).model.playlist.get_current_video();
    let video = room.peer(0).player.state().video;
    let ui = room.peer(0).ui.state();
    for i in 1..room.peers.len() {
        assert_eq!(
            room.peer(i).playlist(),
            playlist,
            "peer {i}: playlist diverged"
        );
        assert_eq!(
            room.peer(i).model.playlist.get_current_video(),
            marker,
            "peer {i}: playlist marker diverged"
        );
        assert_eq!(
            room.peer(i).player.state().video,
            video,
            "peer {i}: loaded video diverged"
        );
        let peer_ui = room.peer(i).ui.state();
        assert_eq!(
            peer_ui.playing_video, ui.playing_video,
            "peer {i}: UI video diverged"
        );
        assert_eq!(
            peer_ui.playlist, ui.playlist,
            "peer {i}: UI playlist diverged"
        );
    }
}

proptest! {
    /// After ANY sequence of user actions — including host handovers and
    /// peers joining mid-session — the room goes quiescent (pump panics on
    /// a message storm) and every peer agrees on the playlist, the loaded
    /// video, the video and playlist shown in the UI, and the playlist
    /// marker. After a few quiet host heartbeats everyone also agrees with
    /// the host on paused/playing, and no video server keeps running when
    /// no provider is announced.
    ///
    /// The marker assertion guards the select_playing fix: keeping a stale
    /// marker on an out-of-playlist Select made present peers disagree with
    /// later joiners, who only get the last Select replayed.
    #[test]
    fn any_dance_converges(actions in prop::collection::vec(action(), 1..40)) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut room = Room::new();
            room.join("alice", POOL);
            room.join("bob", &POOL[..1]);
            room.pump();

            for action in actions {
                apply(&mut room, action);
            }
            room.pump();

            assert_room_converged(&mut room);

            // a few quiet host heartbeats let the reconciliation settle,
            // and must not break the converged state
            for _ in 0..3 {
                let host = room.host_index();
                room.peer(host).heartbeat();
                room.pump();
            }
            assert_room_converged(&mut room);

            // every peer with a loaded video follows the host's play state
            let host_player = room.peer(room.host_index()).player.state();
            if host_player.video.is_some() {
                for i in 0..room.peers.len() {
                    assert_eq!(
                        room.peer(i).player.state().paused,
                        host_player.paused,
                        "peer {i}: paused/playing diverged from the host"
                    );
                }
            }

            // when no provider is announced, nobody keeps a video server
            // running (there is nothing left to stream from)
            if room.provider_index().is_none() {
                for i in 0..room.peers.len() {
                    assert!(
                        !room.peer(i).video_server.running(),
                        "peer {i}: video server running without an announced provider"
                    );
                }
            }
        });
    }
}
