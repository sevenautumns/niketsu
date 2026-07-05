//! Scenarios. Each test name is one sentence of the protocol contract.

use std::time::Duration;

use niketsu_core::player::{PlayerFileEnd, PlayerPause, PlayerSpeedChange, PlayerStart};
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::{ui, video_provider, video_server};

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

/// A client slightly behind the host catches up by playing faster, never by
/// seeking; only a divergence beyond the maximum delay snaps the position.
#[tokio::test]
async fn lagging_client_speeds_up_instead_of_seeking() {
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

    // both play, but bob fell 5 seconds behind (within the catch-up window)
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(95);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let bob_player = room.peer(bob).player.state();
    assert_eq!(
        bob_player.position,
        Duration::from_secs(95),
        "small lag must not cause a seek"
    );
    assert!(
        bob_player.speed > 1.0 && bob_player.speed < 1.15,
        "client should catch up via a bounded speed increase, got {}",
        bob_player.speed
    );

    // bob diverged beyond the maximum delay: position snaps to the host
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(130));
    room.peer(alice).heartbeat();
    room.pump();

    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(100),
        "divergence beyond the maximum delay must hard-sync"
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

    // bob's clock is now the reference: a lagging alice catches up to him
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
    assert!(
        room.peer(alice).player.state().speed > 1.0,
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

/// A peer without the file requests it, the provider answers, and chunks
/// flow from the provider's disk to the requester's video server.
#[tokio::test]
async fn requested_file_is_streamed_chunk_by_chunk() {
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

    // alice shares her local copy of the running video
    room.peer(alice).act(ui::FileShareChange {});
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep1".into(),
        size: 1_000,
    });
    room.pump();

    // bob has no local file and asks the room for it
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(
        room.peer(bob).video_server.running(),
        "provider's file response must start the requester's video server"
    );

    // bob's video server needs the first chunk
    room.peer(bob).act(video_server::ChunkRequest {
        file_name: "ep1".into(),
        start: 0,
        length: 512,
    });
    room.pump();

    // the request arrived at alice's provider, and only there
    let requests = room.peer(alice).provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!((requests[0].start, requests[0].len), (0, 512));

    // alice's provider serves the chunk from disk
    room.peer(alice).act(video_provider::ChunkResponse {
        uuid: requests[0].uuid,
        file_name: "ep1".into(),
        start: 0,
        bytes: vec![7; 512],
    });
    room.pump();

    assert_eq!(
        room.peer(bob).video_server.chunks(),
        vec![(0, vec![7; 512])],
        "the chunk must land in the requester's video server"
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

/// A host speed change propagates to every player, and a lagging client's
/// catch-up speed scales relative to the host speed instead of 1.0.
#[tokio::test]
async fn host_speed_change_propagates_and_scales_the_catchup() {
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

    // bob fell behind while both play at double speed: he catches up by
    // playing faster than the host speed, still within the bounded window
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(95);
    });
    room.peer(alice).heartbeat();
    room.pump();

    let speed = room.peer(bob).player.state().speed;
    assert!(
        speed > 2.0 && speed < 2.0 * 1.15,
        "catch-up must speed up relative to the host speed, got {speed}"
    );

    // back in sync, bob settles on the host speed exactly
    room.peer(bob)
        .player
        .set(|player| player.position = Duration::from_secs(100));
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().speed,
        2.0,
        "in sync, the client must run at the host speed again"
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

/// A speed change arriving mid-catch-up keeps the lagging client faster
/// than the new host speed — the carried-over offset must not flip sign
/// and briefly drop it further behind.
#[tokio::test]
async fn speed_change_mid_catchup_keeps_the_client_faster() {
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

    // bob fell behind and is catching up by playing faster than the host
    room.peer(alice).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(100);
    });
    room.peer(bob).player.set(|player| {
        player.paused = false;
        player.position = Duration::from_secs(95);
    });
    room.peer(alice).heartbeat();
    room.pump();
    assert!(room.peer(bob).player.state().speed > 1.0);

    // mid-ketchup, alice switches to double speed
    room.peer(alice).player.set(|player| player.speed = 2.0);
    room.peer(alice).act(PlayerSpeedChange::new(2.0));
    room.pump();

    let speed = room.peer(bob).player.state().speed;
    assert!(
        speed > 2.0 && speed <= 2.0 + 0.15,
        "a catching-up client must stay faster than the new host speed, got {speed}"
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

/// Chunk responses whose uuid is no longer pending are dropped by the
/// network, not delivered to another consumer's video server.
#[tokio::test]
async fn stale_chunk_responses_are_dropped_not_misrouted() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &[]);
    let charlie = room.join("charlie", &[]);
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

    // both consumers stream from alice and each ask for one chunk
    for who in [bob, charlie] {
        room.peer(who).act(ui::FileRequest {});
    }
    room.pump();
    room.peer(bob).act(video_server::ChunkRequest {
        file_name: "ep1".into(),
        start: 0,
        length: 512,
    });
    room.peer(charlie).act(video_server::ChunkRequest {
        file_name: "ep1".into(),
        start: 512,
        length: 512,
    });
    room.pump();

    let requests = room.peer(alice).provider.requests();
    assert_eq!(requests.len(), 2);

    // alice answers bob's chunk twice: the duplicate's uuid is no longer
    // pending and must be dropped
    for _ in 0..2 {
        room.peer(alice).act(video_provider::ChunkResponse {
            uuid: requests[0].uuid,
            file_name: "ep1".into(),
            start: 0,
            bytes: vec![7; 512],
        });
        room.pump();
    }

    // ...as must a response with a uuid nobody ever asked for
    room.peer(alice).act(video_provider::ChunkResponse {
        uuid: uuid::Uuid::new_v4(),
        file_name: "ep1".into(),
        start: 999,
        bytes: vec![9; 8],
    });
    room.pump();

    // charlie's answer still routes to charlie, and only to charlie
    room.peer(alice).act(video_provider::ChunkResponse {
        uuid: requests[1].uuid,
        file_name: "ep1".into(),
        start: 512,
        bytes: vec![8; 512],
    });
    room.pump();

    assert_eq!(
        room.peer(bob).video_server.chunks(),
        vec![(0, vec![7; 512])],
        "the duplicate and unknown responses must be dropped"
    );
    assert_eq!(
        room.peer(charlie).video_server.chunks(),
        vec![(512, vec![8; 512])],
        "responses must reach exactly their requester"
    );
}
