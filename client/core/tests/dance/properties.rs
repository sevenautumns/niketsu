//! Property tests: the dance fuzzed.
//!
//! Scenarios pin sequences someone thought of; sync bugs live in the
//! orderings nobody chose. proptest generates random sequences of user
//! actions, the harness executes them deterministically, and afterwards
//! the room must satisfy the invariants below. On failure, proptest
//! shrinks to the minimal action sequence that still breaks them.

use std::time::Duration;

use niketsu_core::player::{PlayerPause, PlayerPositionChange, PlayerSpeedChange, PlayerStart};
use niketsu_core::playlist::Playlist;
use niketsu_core::{ui, video_provider};
use proptest::prelude::*;

use crate::harness::Room;

const POOL: &[&str] = &["ep1", "ep2", "ep3"];

const SPEEDS: &[f64] = &[0.5, 1.0, 1.5, 2.0];

/// Everything a user (or their mpv) can do to the room. Peer indices are
/// taken modulo the current peer count when applied.
#[derive(Debug, Clone)]
enum Action {
    ChangePlaylist { peer: usize, videos: Vec<usize> },
    Select { peer: usize, video: usize },
    Seek { peer: usize, secs: u64 },
    Pause { peer: usize },
    Start { peer: usize },
    SpeedChange { peer: usize, speed: usize },
    ReadyToggle { peer: usize },
    ShareToggle { peer: usize },
    FileRequest { peer: usize },
    FileEnd { peer: usize },
    Heartbeat,
    Rehost { target: usize },
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
        (0..8usize, 0..SPEEDS.len()).prop_map(|(peer, speed)| Action::SpeedChange { peer, speed }),
        (0..8usize).prop_map(|peer| Action::ReadyToggle { peer }),
        (0..8usize).prop_map(|peer| Action::ShareToggle { peer }),
        (0..8usize).prop_map(|peer| Action::FileRequest { peer }),
        (0..8usize).prop_map(|peer| Action::FileEnd { peer }),
        Just(Action::Heartbeat),
        (0..8usize).prop_map(|target| Action::Rehost { target }),
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
        Action::SpeedChange { peer, speed } => {
            // mpv already turned the knob before the event reaches the core
            let peer = peer % room.peers.len();
            let speed = SPEEDS[speed];
            room.peer(peer).player.set(|player| player.speed = speed);
            room.peer(peer).act(PlayerSpeedChange::new(speed));
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
            room.peer(peer).reach_end();
        }
        Action::Heartbeat => room.heartbeat(),
        Action::Rehost { target } => {
            let target = target % room.peers.len();
            if target != room.host_index() {
                room.rehost(target);
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

/// Every peer agrees on the playlist, the selection, the loaded video, and
/// what its UI shows.
fn assert_room_converged(room: &mut Room) {
    let playlist = room.peer(0).playlist();
    let selected = room.peer(0).selected();
    let video = room.peer(0).player.state().video;
    assert_eq!(video, selected, "peer 0: the player holds another video");
    let ui = room.peer(0).ui.state();
    for i in 1..room.peers.len() {
        assert_eq!(
            room.peer(i).playlist(),
            playlist,
            "peer {i}: playlist diverged"
        );
        assert_eq!(
            room.peer(i).selected(),
            selected,
            "peer {i}: selection diverged"
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

/// The room shows a user offering a video exactly when it is the selected
/// one and that user's provider has it open: no stale offer, none missing.
fn assert_offers_are_real(room: &mut Room) {
    let host = room.host_index();
    let selected = room.peer(host).selected();
    let mut open = vec![];
    for i in 0..room.peers.len() {
        let peer = room.peer(i);
        let file = peer.model.video_provider.file_name();
        let offers = file.filter(|f| selected.as_ref().is_some_and(|v| v.as_str() == f.as_str()));
        if peer.online && offers.is_some() {
            open.push(peer.name().to_string());
        }
    }
    open.sort();
    for i in 0..room.peers.len() {
        if !room.peer(i).online {
            continue;
        }
        let ui = room.peer(i).ui.state();
        let mut offering: Vec<String> = ui
            .users
            .iter()
            .filter(|u| u.offers.is_some())
            .map(|u| u.name.to_string())
            .collect();
        offering.sort();
        assert_eq!(offering, open, "peer {i}: the offers shown are not real");
        for user in ui.users.iter() {
            if let Some(file) = &user.offers {
                let offered = Some(file.name.as_str());
                let selected = selected.as_ref().map(|v| v.as_str());
                assert_eq!(offered, selected, "peer {i}: stale offer");
            }
        }
    }
}

proptest! {
    /// After ANY sequence of user actions — including the room moving to
    /// another host and peers joining mid-session — the room goes quiescent (pump panics on
    /// a message storm) and every peer agrees on the playlist, the loaded
    /// video, the video and playlist shown in the UI, and the playlist
    /// selection. After a few quiet heartbeats everyone also agrees with
    /// the host on paused/playing, and no video server keeps running when
    /// no provider is announced.
    #[test]
    fn any_dance_converges(actions in prop::collection::vec(action(), 1..40)) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
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

            // a few quiet heartbeats let the reconciliation settle,
            // and must not break the converged state
            for _ in 0..3 {
                room.heartbeat();
                room.pump();
            }
            assert_room_converged(&mut room);

            // every peer plays at the speed the room agreed on, whether it
            // was there when the speed changed or joined afterwards
            let host_speed = room.peer(room.host_index()).player.state().speed;
            for i in 0..room.peers.len() {
                assert_eq!(
                    room.peer(i).player.state().speed,
                    host_speed,
                    "peer {i}: playback speed diverged from the host"
                );
            }

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

            // when nobody shares a file, nobody keeps a video server
            // running (there is nothing left to stream from)
            if !room.anyone_shares() {
                for i in 0..room.peers.len() {
                    assert!(
                        !room.peer(i).video_server.running(),
                        "peer {i}: video server running while nobody shares"
                    );
                }
            }

            // every offer the room shows is one of the selected video, by a
            // user whose provider has it open
            assert_offers_are_real(&mut room);
        });
    }
}
