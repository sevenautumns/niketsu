//! Scenarios. Each test name is one sentence of the protocol contract.

use std::time::Duration;

use niketsu_core::communicator::{SeekMsg, SelectMsg};
use niketsu_core::player::{
    PlayerFileEnd, PlayerPause, PlayerPositionChange, PlayerSpeedChange, PlayerStart,
};
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::RoomName;
use niketsu_core::{ui, video_provider};

use crate::harness::Room;

/// A peer picking a video makes every peer load it from the start, and the
/// playlist selection converges.
#[tokio::test]
async fn selection_by_one_peer_loads_it_everywhere() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &["ep1", "ep2"]);
    room.pump();

    assert!(room.peer(alice).ui.state().is_host);
    assert!(!room.peer(bob).ui.state().is_host);

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        let player = peer.player.state();
        assert_eq!(player.video, Some(Video::from("ep1")));
        assert_eq!(player.position, Duration::ZERO);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
        assert_eq!(
            peer.model.playlist.get_current_video(),
            Some(Video::from("ep1"))
        );
        assert_eq!(peer.ui.state().playing_video, Some(Video::from("ep1")));
    }
}

/// A host that is still loading reports a stale paused=false and a frozen
/// position from the previous video; clients must hold paused instead of
/// following either, and follow again once the host has the file loaded.
#[tokio::test]
async fn clients_hold_paused_while_host_is_still_loading() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    // alice's mpv takes its time opening the file; bob loads instantly
    room.peer(alice).player.set(|player| player.load_lag = true);
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert!(bob_player.file_loaded);
    assert!(bob_player.paused);

    // while loading, the host reports garbage: stale unpaused state and a
    // frozen position from the previous video
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(42);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert!(
        bob_player.paused,
        "client must not start while the host is still loading"
    );
    assert_eq!(
        bob_player.position,
        Duration::ZERO,
        "client must not be yanked to the host's stale position"
    );

    // the host finished loading and plays from the start; bob follows
    room.peer(alice).player.set(|player| {
        player.file_loaded = true;
        player.position = Duration::ZERO;
    });
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert!(!bob_player.paused);
    assert_eq!(bob_player.position, Duration::ZERO);
}

/// A client within the tolerance is left alone — no seek and, above all, no
/// speed correction, which would be audible for as long as it takes to close
/// the gap. Drifting past the tolerance costs one seek.
#[tokio::test]
async fn small_drift_is_tolerated_and_larger_drift_hard_syncs() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // both play, but bob is 2 seconds behind — within the tolerance
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(98);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert_eq!(
        bob_player.position,
        Duration::from_secs(98),
        "a drift within the tolerance must not cause a seek"
    );
    assert_eq!(
        bob_player.speed, 1.0,
        "a drift must never be corrected by changing the speed"
    );

    // bob drifted past the tolerance: one seek puts him back on the host
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(95));
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert_eq!(
        bob_player.position,
        Duration::from_secs(100),
        "a drift past the tolerance must hard-sync"
    );
    assert_eq!(bob_player.speed, 1.0);

    // ...and so does a client that ran ahead
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(110));
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(100),
        "a client ahead of the host must hard-sync too"
    );
}

/// When the provider stops sharing, every consumer's video server stops —
/// nobody keeps streaming from a dead provider.
#[tokio::test]
async fn provider_loss_stops_the_consumers_video_server() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &[]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // alice shares her local copy; the provider task reports the file ready
    room.peer(alice).act(ui::FileShareChange {});
    assert!(room.peer(alice).model.video_provider.sharing());
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep1".into(),
        size: 1_000,
    });
    room.pump();
    assert!(room.peer(alice).ui.state().video_share);

    // bob streams alice's copy through his local video server
    room.peer(bob)
        .model
        .video_server
        .start_server("ep1".into(), 1_000);
    assert!(room.peer(bob).video_server.running());

    // alice's provider dies, e.g. the shared file disappeared
    room.peer(alice).act(video_provider::SharingStopped);
    room.pump();

    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        !room.peer(bob).video_server.running(),
        "consumer must stop streaming from a dead provider"
    );
}

/// When a video ends, whoever's player finishes first advances the shared
/// playlist for everyone; the end of the playlist unloads everywhere.
#[tokio::test]
async fn file_end_advances_the_playlist_everywhere() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &["ep1", "ep2"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // alice's mpv reaches the end of ep1 first
    room.peer(alice).act(PlayerFileEnd(Video::from("ep1")));
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.player.state().video, Some(Video::from("ep2")));
        assert_eq!(peer.player.state().position, Duration::ZERO);
        assert_eq!(
            peer.model.playlist.get_current_video(),
            Some(Video::from("ep2"))
        );
    }

    // this time bob's mpv ends first — and ep2 was the last video
    room.peer(bob).act(PlayerFileEnd(Video::from("ep2")));
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.player.state().video, None);
        assert_eq!(peer.model.playlist.get_current_video(), None);
        assert_eq!(peer.ui.state().playing_video, None);
    }
}

/// After a host handover, the new host's clock is the position authority
/// and the old host's heartbeats move nobody.
#[tokio::test]
async fn host_handover_moves_the_position_authority() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    let new_host = room.peer(bob).name();
    room.peer(alice).act(ui::HostHandover { new_host });
    room.pump();

    assert_eq!(room.host_index(), bob);
    assert!(!room.peer(alice).ui.state().is_host);
    assert!(room.peer(bob).ui.state().is_host);

    // bob's clock is now the reference: a drifting alice syncs to him
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(95);
    });
    room.peer(bob).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(alice).player.state().position,
        Duration::from_secs(100),
        "after handover, clients must follow the new host's clock"
    );

    // ...and the old host's heartbeats have no authority anymore
    room.peer(alice)
        .player
        .set(|player| player.position = Duration::from_secs(200));
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(100),
        "the old host's clock must not move anyone"
    );
}

/// A peer joining mid-session is greeted with the room's playlist and the
/// running video at the host's current position, and starts playing on the
/// next heartbeat.
#[tokio::test]
async fn late_joiner_gets_playlist_selection_and_position() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // the session is 100 seconds in when charlie joins
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let charlie = room.join("charlie", &["ep1", "ep2"]);
    room.pump();

    let peer = room.peer(charlie);
    assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
    assert_eq!(
        peer.model.playlist.get_current_video(),
        Some(Video::from("ep1"))
    );
    let player = peer.player.state();
    assert_eq!(player.video, Some(Video::from("ep1")));
    assert_eq!(
        player.position,
        Duration::from_secs(100),
        "joiner must start where the host currently is, not at zero"
    );

    // the next host heartbeat starts the (still paused) joiner
    room.peer(alice).heartbeat();
    room.pump();
    assert!(!room.peer(charlie).player.state().paused);
}

/// A peer joining a session that already runs at another speed adopts it
/// from the host's heartbeat: the speed change itself was announced long
/// before the joiner existed.
#[tokio::test]
async fn late_joiner_adopts_the_running_speed() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // the room has been running at 1.5x, 100 seconds in
    room.peer(alice).player.set(|player| player.speed = 1.5);
    room.peer(alice).act(PlayerSpeedChange::new(1.5));
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let charlie = room.join("charlie", &["ep1"]);
    room.pump();
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(charlie).player.state().speed,
        1.5,
        "a joiner must play at the speed the room is running at, not at 1.0"
    );

    // the joiner reconciles against that speed too, so it tolerates the
    // same drift as everyone else instead of being hard-synced constantly
    room.peer(charlie).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(97);
    });
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(charlie).player.state().position,
        Duration::from_secs(97),
        "the joiner's tolerance must scale with the room's speed"
    );
}

/// One peer pausing pauses everyone; when the last unready user readies up,
/// the host auto-starts playback for the whole room.
#[tokio::test]
async fn pause_propagates_and_all_ready_auto_starts() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // bob unpauses his mpv: everyone starts, and bob counts as ready
    room.peer(bob).player.set(|player| player.paused = false);
    room.peer(bob).act(PlayerStart);
    room.pump();
    assert!(
        !room.peer(alice).player.state().paused,
        "start must propagate"
    );

    // alice pauses her mpv: everyone pauses. Only alice drops out of
    // ready — mpv does not re-emit a pause the core commanded (echo
    // suppression in niketsu-mpv), so bob's ready flag survives.
    room.peer(alice).player.set(|player| player.paused = true);
    room.peer(alice).act(PlayerPause);
    room.pump();
    assert!(room.peer(bob).player.state().paused, "pause must propagate");

    // alice readies up again: every user is ready now, so the host
    // publishes an auto-start and both players resume
    let name = room.peer(alice).name();
    room.peer(alice).act(ui::UserChange { name, ready: true });
    room.pump();
    for who in [alice, bob] {
        assert!(
            !room.peer(who).player.state().paused,
            "all users ready must auto-start playback"
        );
    }
}

/// A host speed change propagates to every player, and the tolerance scales
/// with it: positions are media time, so at double speed twice as much of a
/// gap amounts to the same wait for the viewer.
#[tokio::test]
async fn host_speed_change_propagates_and_scales_the_tolerance() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // alice's mpv switches to double speed
    room.peer(alice).player.set(|player| player.speed = 2.0);
    room.peer(alice).act(PlayerSpeedChange::new(2.0));
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().speed,
        2.0,
        "speed change must propagate"
    );

    // 4 seconds apart is beyond the tolerance at 1x, but not at 2x
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(96);
    });
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(96),
        "the tolerance must scale with the speed the room plays at"
    );

    // 7 seconds is beyond it even at double speed
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(93));
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert_eq!(bob_player.position, Duration::from_secs(100));
    assert_eq!(
        bob_player.speed, 2.0,
        "hard-syncing must leave the room's speed alone"
    );
}

/// A client-initiated speed change is adopted by the initiator as its own
/// reference speed too: the next host heartbeat must not drag the initiator
/// back to the old speed while everyone else keeps the new one.
#[tokio::test]
async fn client_speed_change_survives_the_next_heartbeat() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // bob — a client, not the host — switches his mpv to 1.5x
    room.peer(bob).player.set(|player| player.speed = 1.5);
    room.peer(bob).act(PlayerSpeedChange::new(1.5));
    room.pump();
    assert_eq!(
        room.peer(alice).player.state().speed,
        1.5,
        "speed change must propagate"
    );

    // both play in sync; the next host heartbeat reconciles bob against
    // the host clock and must leave his speed alone
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(bob).player.state().speed,
        1.5,
        "the initiator must not be dragged back to the old speed"
    );
}

/// With auto-share on, selecting the next video carries the announced share
/// over to it; selecting one the provider does not have stops the share and
/// every consumer's video server with it.
#[tokio::test]
async fn auto_share_carries_the_share_to_the_next_selection() {
    let mut room = Room::new();
    let alice = room.join_with_auto_share("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &[]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2", "ep3"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    room.peer(alice).act(ui::FileShareChange {});
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep1".into(),
        size: 1_000,
    });
    room.pump();

    // alice selects the next episode she also has: the share follows
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();
    assert_eq!(
        room.peer(alice).model.video_provider.file_name().as_deref(),
        Some("ep2"),
        "auto-share must carry the share to the new selection"
    );
    assert!(room.peer(alice).ui.state().video_share);

    // bob streams the reshared episode
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(room.peer(bob).video_server.running());

    // ep3 is in the playlist but not in alice's file database: the share
    // stops, the room is told, and bob's video server stops with it
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep3"),
    });
    room.pump();
    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        !room.peer(bob).video_server.running(),
        "consumers must not keep streaming a share that ended"
    );
}

/// When the sharing peer's player finishes a video, advancing to the next
/// one runs the same choreography as a manual selection: auto-share carries
/// the share over, and the end of the playlist stops it — on the finishing
/// peer just like on everyone who receives its Select.
#[tokio::test]
async fn file_end_reshares_or_stops_like_a_manual_selection() {
    let mut room = Room::new();
    let alice = room.join_with_auto_share("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &[]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    room.peer(alice).act(ui::FileShareChange {});
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep1".into(),
        size: 1_000,
    });
    room.pump();

    // alice's mpv finishes ep1: the share must follow to ep2 exactly as it
    // would on a manual selection
    room.peer(alice).act(PlayerFileEnd(Video::from("ep1")));
    room.pump();
    assert_eq!(
        room.peer(alice).model.video_provider.file_name().as_deref(),
        Some("ep2"),
        "file-end must carry the auto-share to the next video"
    );
    assert!(room.peer(alice).ui.state().video_share);

    // bob streams the reshared episode
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(room.peer(bob).video_server.running());

    // ep2 was the last video: the share stops and bob's server with it
    room.peer(alice).act(PlayerFileEnd(Video::from("ep2")));
    room.pump();
    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        !room.peer(bob).video_server.running(),
        "the end of the playlist must stop the finishing peer's share"
    );
}

/// A file request when nobody announces the file finds no provider: the
/// requester is told and its video server stops instead of waiting for
/// chunks that will never come.
#[tokio::test]
async fn file_request_without_a_provider_stops_the_video_server() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &[]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // bob's video server is still up from an earlier stream
    room.peer(bob)
        .model
        .video_server
        .start_server("ep1".into(), 1_000);
    assert!(room.peer(bob).video_server.running());

    // nobody shares ep1, so the lookup finds no provider
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(
        !room.peer(bob).video_server.running(),
        "a request without a provider must stop the requester's video server"
    );
}

/// A failed file response — the announced provider is not actually serving
/// the file — stops the requester's video server instead of leaving it up
/// serving a stream nobody will ever feed.
#[tokio::test]
async fn failed_file_response_stops_the_requesters_video_server() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &[]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    room.peer(alice).act(ui::FileShareChange {});
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep1".into(),
        size: 1_000,
    });
    room.pump();

    // bob streams ep1 from alice
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(room.peer(bob).video_server.running());

    // alice's provider dies without the room noticing (a stale provider
    // record): bob's retry still reaches her and is answered with a
    // failure, which must take his dead stream down with it
    room.peer(alice).model.video_provider.stop_providing();
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(
        !room.peer(bob).video_server.running(),
        "a failed file response must stop the requester's video server"
    );
}

fn users(names: &[(&str, bool)]) -> Vec<(String, bool)> {
    names.iter().map(|(n, r)| (n.to_string(), *r)).collect()
}

/// Everyone, late joiners included, sees the same user list, and the host
/// keeps it current as users come, ready up and leave.
#[tokio::test]
async fn the_user_list_follows_joins_ready_changes_and_departures() {
    let mut room = Room::new();
    let alice = room.join("alice", &[]);
    let bob = room.join("bob", &[]);
    room.pump();

    let charlie = room.join("charlie", &[]);
    room.pump();
    let everyone = users(&[("alice", false), ("bob", false), ("charlie", false)]);
    for who in [alice, bob, charlie] {
        assert_eq!(room.peer(who).ui.state().user_names(), everyone);
    }

    let name = room.peer(bob).name();
    room.peer(bob).act(ui::UserChange { name, ready: true });
    room.pump();
    let bob_ready = users(&[("alice", false), ("bob", true), ("charlie", false)]);
    for who in [alice, bob, charlie] {
        assert_eq!(room.peer(who).ui.state().user_names(), bob_ready);
    }

    room.leave(charlie);
    room.pump();
    let without_charlie = users(&[("alice", false), ("bob", true)]);
    for who in [alice, bob] {
        assert_eq!(
            room.peer(who).ui.state().user_names(),
            without_charlie,
            "a departed user must drop out of everyone's list"
        );
    }
}

/// A joiner whose name is taken is renamed by the host, and the rename
/// reaches the joiner itself.
#[tokio::test]
async fn the_host_resolves_username_collisions() {
    let mut room = Room::new();
    let alice = room.join("alice", &[]);
    let bob = room.join("bob", &[]);
    room.pump();

    let impostor = room.join("bob", &[]);
    room.pump();

    let renamed = room.peer(impostor).name();
    assert_ne!(renamed, "bob", "the joiner must not keep a taken name");
    assert!(renamed.starts_with("bob_"));
    assert_eq!(room.peer(bob).name(), "bob", "the first bob keeps his name");

    let expected = users(&[("alice", false), ("bob", false), (renamed.as_str(), false)]);
    for who in [alice, bob, impostor] {
        assert_eq!(room.peer(who).ui.state().user_names(), expected);
    }

    // renaming into a taken name is resolved the same way
    room.peer(bob).act(ui::UserChange {
        name: "alice".into(),
        ready: false,
    });
    room.pump();
    let bob_now = room.peer(bob).name();
    assert!(bob_now.starts_with("alice_"), "got {bob_now}");
    assert_eq!(room.peer(alice).name(), "alice");
    assert_eq!(room.peer(alice).ui.state().users.len(), 3);
}

/// When the last user readies up — here a client, so the decision is the
/// host's reaction to a peer's status — playback starts for everyone.
#[tokio::test]
async fn a_client_readying_up_last_auto_starts_the_room() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    let name = room.peer(alice).name();
    room.peer(alice).act(ui::UserChange { name, ready: true });
    room.pump();
    for who in [alice, bob] {
        assert!(
            room.peer(who).player.state().paused,
            "one ready user out of two must not start anything"
        );
    }

    let name = room.peer(bob).name();
    room.peer(bob).act(ui::UserChange { name, ready: true });
    room.pump();
    for who in [alice, bob] {
        assert!(
            !room.peer(who).player.state().paused,
            "the last user readying up must start the room"
        );
    }
}

/// A client removing the running video from the playlist makes the host
/// move the whole room on to the video that took its place.
#[tokio::test]
async fn removing_the_running_video_selects_its_successor() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2", "ep3"]);
    let bob = room.join("bob", &["ep1", "ep2", "ep3"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2", "ep3"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();

    // bob deletes ep2 while it runs
    let playlist = Playlist::from_iter(["ep1", "ep3"]);
    room.peer(bob).ui.set(|ui| ui.playlist = playlist.clone());
    room.peer(bob).act(ui::PlaylistChange { playlist });
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep3"]));
        assert_eq!(peer.player.state().video, Some(Video::from("ep3")));
        assert_eq!(
            peer.model.playlist.get_current_video(),
            Some(Video::from("ep3"))
        );
    }

    // a late joiner gets the same picture
    let charlie = room.join("charlie", &[]);
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().video,
        Some(Video::from("ep3"))
    );
}

/// After a seek arrives, a client ignores the host's clock until its own
/// player has moved: a heartbeat the host sent before applying the seek
/// would otherwise drag the client back to where the room was.
#[tokio::test]
async fn a_client_ignores_the_host_clock_until_its_seek_has_landed() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    let charlie = room.join("charlie", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();
    for who in [alice, bob, charlie] {
        room.peer(who).player.set(|player| {
            player.paused = false;
            player.position = Duration::from_secs(10);
        });
    }
    room.heartbeat();
    room.pump();

    // bob seeks; charlie receives it, alice (the host) too
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(300));
    room.peer(bob)
        .act(PlayerPositionChange::new(Duration::from_secs(300)));
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().position,
        Duration::from_secs(300)
    );

    // a stale host heartbeat, as if sent before the host applied the seek
    room.peer(alice)
        .player
        .set(|player| player.position = Duration::from_secs(10));
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().position,
        Duration::from_secs(300),
        "a client must not be yanked back while its seek settles"
    );

    // charlie's player moved on; from now on the host's clock counts again
    room.peer(charlie)
        .player
        .set(|player| player.position = Duration::from_secs(301));
    room.peer(charlie).heartbeat();
    room.peer(alice)
        .player
        .set(|player| player.position = Duration::from_secs(320));
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().position,
        Duration::from_secs(320)
    );
}

/// The host's position is half a round trip old when it arrives; a playing
/// client reconciles against where the host is by now, a paused one against
/// the reported position as is.
#[tokio::test]
async fn clients_compensate_the_host_position_for_the_network_delay() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // a quiet ping of 400ms and one congested one, which must not count
    room.round_trip(bob, Duration::from_millis(400));
    room.round_trip(bob, Duration::from_secs(3));

    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(90);
    });
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_millis(100_200),
        "a playing host is half the quietest round trip further by now"
    );

    // 2.6s ahead of the reported position, but only 2.4s ahead of where
    // the host is by now: within the tolerance
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_millis(102_600));
    room.peer(bob).heartbeat();
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_millis(102_600)
    );

    // a paused host does not move while the message travels
    room.peer(alice).player.set(|player| player.paused = true);
    room.peer(bob).player.set(|player| {
        player.paused = true;
        player.position = Duration::from_secs(90);
    });
    room.peer(bob).heartbeat();
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(100)
    );
}

/// A handover keeps the room's state: the new host does not restart the
/// running video, and everyone who reconnects to it — the old host
/// included — is greeted with the playlist, the video at the new host's
/// position, and the full user list.
#[tokio::test]
async fn a_handover_keeps_the_room_running() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &["ep1", "ep2"]);
    let charlie = room.join("charlie", &["ep1", "ep2"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();
    for who in [alice, bob, charlie] {
        room.peer(who).player.set(|player| {
            player.paused = false;
            player.position = Duration::from_secs(60);
        });
    }

    let new_host = room.peer(bob).name();
    room.peer(alice).act(ui::HostHandover { new_host });
    room.pump();

    assert_eq!(room.host_index(), bob);
    let bob_player = room.peer(bob).player.state();
    assert_eq!(bob_player.video, Some(Video::from("ep2")));
    assert_eq!(
        bob_player.position,
        Duration::from_secs(60),
        "the new host must not restart the running video"
    );
    let everyone = users(&[("alice", false), ("bob", false), ("charlie", false)]);
    for who in [alice, bob, charlie] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
        assert_eq!(peer.player.state().video, Some(Video::from("ep2")));
        assert_eq!(peer.player.state().position, Duration::from_secs(60));
        assert_eq!(peer.ui.state().user_names(), everyone);
    }
}

/// The playlist of an earlier session: ep1 and ep2, with ep2 playing.
fn saved_playlist(_: &RoomName) -> Option<PlaylistHandler> {
    let mut saved = PlaylistHandler::default();
    saved.replace(Playlist::from_iter(["ep1", "ep2"]));
    saved.select_playing(&Video::from("ep2"));
    Some(saved)
}

/// A host starting a room fresh restores the room's saved playlist and
/// selection, and peers joining later are greeted with it.
#[tokio::test]
async fn a_fresh_host_restores_the_rooms_saved_playlist() {
    let mut room = Room::with_saved_playlist(saved_playlist);
    let alice = room.join("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &["ep1", "ep2"]);
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
        assert_eq!(peer.player.state().video, Some(Video::from("ep2")));
        assert_eq!(peer.player.state().position, Duration::ZERO);
        assert_eq!(peer.ui.state().playing_video, Some(Video::from("ep2")));
    }
}

/// A core that already followed the room keeps the room's state when it
/// becomes host, whatever is saved for the room.
#[tokio::test]
async fn a_new_host_of_the_same_room_ignores_the_saved_playlist() {
    let mut room = Room::with_saved_playlist(saved_playlist);
    let alice = room.join("alice", &["ep1", "ep2", "ep3"]);
    let bob = room.join("bob", &["ep1", "ep2", "ep3"]);
    room.pump();
    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep3"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep3"),
    });
    room.pump();

    let new_host = room.peer(bob).name();
    room.peer(alice).act(ui::HostHandover { new_host });
    room.pump();

    assert_eq!(room.host_index(), bob);
    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep3"]));
        assert_eq!(peer.player.state().video, Some(Video::from("ep3")));
    }
}

/// Only the host can hand the room over; a client asking for it sends
/// nothing, and nobody reconnects.
#[tokio::test]
async fn a_client_cannot_hand_over_the_room() {
    let mut room = Room::new();
    let alice = room.join("alice", &[]);
    let bob = room.join("bob", &[]);
    let charlie = room.join("charlie", &[]);
    room.pump();

    let new_host = room.peer(charlie).name();
    room.peer(bob).act(ui::HostHandover { new_host });
    room.pump();

    assert_eq!(room.host_index(), alice);
    assert!(room.peer(alice).ui.state().is_host);
    assert!(!room.peer(charlie).ui.state().is_host);
}

/// A client that ended up on another video than the host — it lost a race
/// between a selection and a seek — is brought back by the host's
/// heartbeats, once they named the host's video often enough not to be
/// heartbeats that merely crossed a selection.
#[tokio::test]
async fn a_client_on_another_video_follows_the_host_after_a_few_heartbeats() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    let bob = room.join("bob", &["ep1", "ep2"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();

    // bob alone takes a selection of ep1, as if it had crossed alice's
    room.peer(bob).act(SelectMsg {
        actor: "charlie".into(),
        position: Duration::ZERO,
        video: Some(Video::from("ep1")),
    });
    room.pump();
    room.peer(alice)
        .player
        .set(|player| player.position = Duration::from_secs(40));

    for _ in 0..2 {
        room.heartbeat();
        room.pump();
    }
    assert_eq!(
        room.peer(bob).player.state().video,
        Some(Video::from("ep1")),
        "a heartbeat or two may just have crossed a selection"
    );

    room.heartbeat();
    room.pump();
    let player = room.peer(bob).player.state();
    assert_eq!(player.video, Some(Video::from("ep2")));
    assert_eq!(player.position, Duration::from_secs(40));
    assert_eq!(
        room.peer(bob).model.playlist.get_current_video(),
        Some(Video::from("ep2"))
    );
}

/// A seek can name another video than the one the host plays, when it
/// crossed a selection on the way. The host follows it like every peer,
/// and greets a peer joining afterwards with the video the room plays.
#[tokio::test]
async fn a_seek_into_another_video_is_what_a_late_joiner_is_greeted_with() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    room.pump();

    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    room.peer(alice).act(SeekMsg {
        actor: "bob".into(),
        video: Video::from("ep2"),
        position: Duration::from_secs(300),
    });
    room.pump();
    assert_eq!(
        room.peer(alice).player.state().video,
        Some(Video::from("ep2"))
    );

    let charlie = room.join("charlie", &["ep1", "ep2"]);
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().video,
        Some(Video::from("ep2")),
        "a late joiner must load the video the host plays"
    );
}
