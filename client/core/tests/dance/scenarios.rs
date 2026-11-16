//! Scenarios. Each test name is one sentence of the protocol contract.

use std::time::Duration;

use niketsu_core::chunk::SharedFile;
use niketsu_core::communicator::{DisconnectedMsg, Intent, PeerIntentMsg, RoomStateMsg};
use niketsu_core::player::{
    PlayerPause, PlayerPositionChange, PlayerSpeedChange, PlayerStart, Source,
};
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::RoomName;
use niketsu_core::{ui, video_provider};

use crate::harness::{FILE_SIZE, Peer, Room};

const SECOND: Duration = Duration::from_secs(1);

/// A user starts playback in mpv, which plays before anyone else knows.
fn start(room: &mut Room, who: usize) {
    room.peer(who).player.set(|player| player.paused = false);
    room.peer(who).act(PlayerStart);
}

/// A user pauses in mpv, which is paused before anyone else knows.
fn pause(room: &mut Room, who: usize) {
    room.peer(who).player.set(|player| player.paused = true);
    room.peer(who).act(PlayerPause);
}

/// The player of `who` ends up at `secs` without telling anyone.
fn drift(room: &mut Room, who: usize, secs: u64) {
    put(room, who, Duration::from_secs(secs));
}

/// The player of `who` ends up at `position` without telling anyone.
fn put(room: &mut Room, who: usize, position: Duration) {
    room.peer(who)
        .player
        .set(|player| player.position = position);
}

fn position(room: &mut Room, who: usize) -> Duration {
    room.peer(who).player.state().position
}

/// A peer picking a video makes every peer load it from the start, and the
/// playlist selection converges.
#[tokio::test(start_paused = true)]
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
        assert_eq!(peer.selected(), Some(Video::from("ep1")));
        assert_eq!(peer.ui.state().playing_video, Some(Video::from("ep1")));
    }
}

/// A room of ready users starts a newly selected video once everyone has
/// it loaded, not before: nobody plays ahead of a player that is still
/// opening the file.
#[tokio::test(start_paused = true)]
async fn the_room_does_not_start_until_everyone_has_loaded() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();
    for who in [alice, bob] {
        let name = room.peer(who).name();
        room.peer(who).act(ui::UserChange { name, ready: true });
    }
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
    room.pass(3 * SECOND).await;
    room.heartbeat();
    room.pump();

    assert!(room.peer(bob).player.state().file_loaded);
    for who in [alice, bob] {
        let player = room.peer(who).player.state();
        assert!(player.paused, "peer {who} plays before alice has loaded");
        assert_eq!(player.position, Duration::ZERO);
    }

    room.peer(alice).player.finish_load();
    room.pump();

    for who in [alice, bob] {
        assert!(!room.peer(who).player.state().paused, "peer {who}");
    }
    room.pass(5 * SECOND).await;
    for who in [alice, bob] {
        assert_eq!(position(&mut room, who), 5 * SECOND);
    }
}

/// A player within the tolerance is left alone — no seek and, above all, no
/// speed correction, which would be audible for as long as it takes to close
/// the gap. Being off by more costs one seek: at once for a player behind
/// the room, and for one that looks ahead of it once it has looked so for a
/// few seconds, since a late room state makes a level player look ahead.
/// The host's player is corrected like a client's.
#[tokio::test(start_paused = true)]
async fn a_small_offset_is_tolerated_and_a_larger_one_costs_one_seek() {
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
    start(&mut room, alice);
    room.pump();
    room.watch(100 * SECOND).await;
    let mut at = 100 * SECOND;
    let tick = SECOND / 2;

    for who in [alice, bob] {
        // 0.4 seconds behind the room — within the tolerance
        put(&mut room, who, at - SECOND * 2 / 5);
        room.watch(5 * SECOND).await;
        at += 5 * SECOND;

        let player = room.peer(who).player.state();
        assert_eq!(
            player.position,
            at - SECOND * 2 / 5,
            "an offset within the tolerance must not cause a seek"
        );
        assert_eq!(
            player.speed, 1.0,
            "an offset must never be corrected by changing the speed"
        );

        // fallen behind by more: one seek puts the player back
        put(&mut room, who, at - 2 * SECOND);
        room.watch(tick).await;
        at += tick;

        let player = room.peer(who).player.state();
        assert_eq!(player.position, at, "a player behind must hard-sync");
        assert_eq!(player.speed, 1.0);

        // a player ahead may only look so for a late state
        put(&mut room, who, at + 2 * SECOND);
        room.watch(3 * SECOND).await;
        at += 3 * SECOND;
        assert_eq!(
            position(&mut room, who),
            at + 2 * SECOND,
            "a player that looks ahead is watched before it is moved"
        );

        // ...and is put back when it is ahead for as long as it is watched
        room.watch(tick).await;
        at += tick;
        assert_eq!(
            position(&mut room, who),
            at,
            "a player ahead of the room must hard-sync too"
        );
    }
}

/// Whether the player of `who` reads the video from another peer.
fn streams(room: &mut Room, who: usize) -> bool {
    room.peer(who).player.stream().is_some()
}

/// When the provider stops sharing, the stream of every viewer fails:
/// nobody keeps a player waiting for a dead provider.
#[tokio::test(start_paused = true)]
async fn provider_loss_fails_the_viewers_stream() {
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

    // bob's player reads alice's copy
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert_eq!(room.provider_of(bob), Some(alice));
    let bob_id = room.peers[bob].id;
    let host_users = room.peer(alice).model.state.users.clone();
    assert_eq!(host_users[&bob_id].loaded, Some(Video::from("ep1")));

    // alice's provider dies, e.g. the shared file disappeared
    room.peer(alice).act(video_provider::SharingStopped);
    room.pump();

    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        room.peer(bob).player.state().source.is_none(),
        "a viewer must not keep streaming from a dead provider"
    );
    assert!(offering(&mut room, bob).is_empty(), "the offer stays shown");
    // the room knows that bob has the video no longer
    let host_users = room.peer(alice).model.state.users.clone();
    assert_eq!(host_users[&bob_id].loaded, None);
    assert_eq!(room.peer(bob).selected(), Some(Video::from("ep1")));
}

/// When a video ends, whoever's player finishes first advances the shared
/// playlist for everyone; the end of the playlist unloads everywhere.
#[tokio::test(start_paused = true)]
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
    room.peer(alice).reach_end();
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.player.state().video, Some(Video::from("ep2")));
        assert_eq!(peer.player.state().position, Duration::ZERO);
        assert_eq!(peer.selected(), Some(Video::from("ep2")));
    }

    // this time bob's mpv ends first — and ep2 was the last video
    room.peer(bob).reach_end();
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.player.state().video, None);
        assert_eq!(peer.selected(), None);
        assert_eq!(peer.ui.state().playing_video, None);
    }
}

/// Every player reaches the end of a video, one after the other. The room
/// moves on to the next video once, and the later ends neither skip that
/// video nor start it over.
#[tokio::test(start_paused = true)]
async fn two_peers_reaching_the_end_advance_the_playlist_once() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2", "ep3"]);
    let bob = room.join("bob", &["ep1", "ep2", "ep3"]);
    room.pump();
    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2", "ep3"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();

    // alice's player ends first. bob's ends before he hears of ep2, and
    // the room is 40 seconds into ep2 when the end of ep1 on his reaches
    // the room
    room.peer(alice).reach_end();
    seek(&mut room, alice, 40);
    room.peer(bob).reach_end();
    room.pump();

    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.selected(), Some(Video::from("ep2")), "peer {who}");
        let player = peer.player.state();
        assert_eq!(player.video, Some(Video::from("ep2")), "peer {who}");
        assert_eq!(player.position, 40 * SECOND, "peer {who}");
    }
    let selections = |line: &&String| line.contains("selected");
    let actions = room.peer(alice).ui.state().actions;
    assert_eq!(
        actions.iter().filter(selections).collect::<Vec<_>>(),
        ["alice selected ep1", "alice selected ep2"]
    );
}

/// When the room moves to another host, the new host's clock is the
/// room's, and the old host follows it like any client.
#[tokio::test(start_paused = true)]
async fn a_new_host_is_the_position_authority() {
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

    room.rehost(bob);
    room.pump();

    assert_eq!(room.host_index(), bob);
    assert!(!room.peer(alice).ui.state().is_host);
    assert!(room.peer(bob).ui.state().is_host);

    // the room plays by bob's clock now: a drifting alice is corrected
    start(&mut room, bob);
    room.pump();
    room.watch(100 * SECOND).await;
    drift(&mut room, alice, 95);
    room.watch(SECOND).await;
    assert_eq!(
        position(&mut room, alice),
        101 * SECOND,
        "the old host must follow the new host's clock"
    );

    // ...and where the old host's player is moves nobody
    drift(&mut room, alice, 200);
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(
        position(&mut room, bob),
        101 * SECOND,
        "the old host's player must not move anyone"
    );
}

/// A peer joining mid-session is greeted with the room's playlist and the
/// running video where the room is in it, and plays along.
#[tokio::test(start_paused = true)]
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
    start(&mut room, alice);
    room.pump();
    room.pass(100 * SECOND).await;

    let charlie = room.join("charlie", &["ep1", "ep2"]);
    room.pump();

    let peer = room.peer(charlie);
    assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
    assert_eq!(peer.selected(), Some(Video::from("ep1")));
    let player = peer.player.state();
    assert_eq!(player.video, Some(Video::from("ep1")));
    assert_eq!(
        player.position,
        100 * SECOND,
        "joiner must start where the room currently is, not at zero"
    );
    assert!(!player.paused, "the joiner plays along with the room");
}

/// A peer joining right after the host seeks starts at the seek, though no
/// heartbeat has carried it yet.
#[tokio::test(start_paused = true)]
async fn late_joiner_right_after_a_seek_starts_there() {
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

    seek(&mut room, alice, 600);

    let charlie = room.join("charlie", &["ep1"]);
    room.pump();

    assert_eq!(
        position(&mut room, charlie),
        600 * SECOND,
        "joiner must start where the room is, not at its last heartbeat"
    );
}

/// A peer joining a session that already runs at another speed adopts it
/// from the room's state: the speed change itself was announced long
/// before the joiner existed.
#[tokio::test(start_paused = true)]
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

    // the room has been running at 1.5x for 100 seconds
    room.peer(alice).player.set(|player| player.speed = 1.5);
    room.peer(alice).act(PlayerSpeedChange::new(1.5));
    start(&mut room, alice);
    room.pump();
    room.pass(100 * SECOND).await;

    let charlie = room.join("charlie", &["ep1"]);
    room.pump();

    let player = room.peer(charlie).player.state();
    assert_eq!(
        player.speed, 1.5,
        "a joiner must play at the speed the room is running at, not at 1.0"
    );
    assert_eq!(player.position, 150 * SECOND);

    // the joiner is judged at that speed too: 0.6 seconds behind is beyond
    // the tolerance at 1x, but not at 1.5x
    put(&mut room, charlie, Duration::from_millis(149_400));
    room.watch(5 * SECOND).await;

    assert_eq!(
        position(&mut room, charlie),
        Duration::from_millis(156_900),
        "the joiner's tolerance must scale with the room's speed"
    );
}

/// One peer pausing pauses everyone; when the last unready user readies up,
/// the host auto-starts playback for the whole room.
#[tokio::test(start_paused = true)]
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
#[tokio::test(start_paused = true)]
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

    // 0.8 seconds behind is beyond the tolerance at 1x, but not at 2x
    start(&mut room, alice);
    room.pump();
    room.watch(50 * SECOND).await;
    assert_eq!(position(&mut room, alice), 100 * SECOND);
    put(&mut room, bob, Duration::from_millis(99_200));
    room.watch(5 * SECOND).await;

    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_millis(109_200),
        "the tolerance must scale with the speed the room plays at"
    );

    // 1.5 seconds is beyond it even at double speed
    put(&mut room, bob, Duration::from_millis(108_500));
    room.watch(SECOND / 2).await;

    let bob_player = room.peer(bob).player.state();
    assert_eq!(bob_player.position, Duration::from_secs(111));
    assert_eq!(
        bob_player.speed, 2.0,
        "hard-syncing must leave the room's speed alone"
    );
}

/// A client-initiated speed change is adopted by the initiator as its own
/// reference speed too: the next host heartbeat must not drag the initiator
/// back to the old speed while everyone else keeps the new one.
#[tokio::test(start_paused = true)]
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

    // both play in sync; the host's heartbeats judge bob against the
    // room's clock and must leave his speed alone
    start(&mut room, alice);
    room.pump();
    room.watch(100 * SECOND).await;

    assert_eq!(
        room.peer(bob).player.state().speed,
        1.5,
        "the initiator must not be dragged back to the old speed"
    );
}

/// With auto-share on, selecting the next video carries the announced share
/// over to it; selecting one the provider does not have stops the share and
/// every viewer's stream with it.
#[tokio::test(start_paused = true)]
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
    // the provider task reports the reshared file ready, which offers it
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep2".into(),
        size: 1_000,
    });
    room.pump();

    // bob streams the reshared episode
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(streams(&mut room, bob));

    // ep3 is in the playlist but not in alice's file database: the share
    // stops, the room is told, and bob's stream ends with it
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep3"),
    });
    room.pump();
    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        !streams(&mut room, bob),
        "viewers must not keep streaming a share that ended"
    );
}

/// When the sharing peer's player finishes a video, advancing to the next
/// one runs the same choreography as a manual selection: auto-share carries
/// the share over, and the end of the playlist stops it — on the finishing
/// peer just like on everyone who receives its Select.
#[tokio::test(start_paused = true)]
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
    room.peer(alice).reach_end();
    room.pump();
    assert_eq!(
        room.peer(alice).model.video_provider.file_name().as_deref(),
        Some("ep2"),
        "file-end must carry the auto-share to the next video"
    );
    assert!(room.peer(alice).ui.state().video_share);
    // the provider task reports the reshared file ready, which offers it
    room.peer(alice).act(video_provider::FileReady {
        file_name: "ep2".into(),
        size: 1_000,
    });
    room.pump();

    // bob streams the reshared episode
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    assert!(streams(&mut room, bob));

    // ep2 was the last video: the share stops and bob's stream with it
    room.peer(alice).reach_end();
    room.pump();
    assert!(!room.peer(alice).model.video_provider.sharing());
    assert!(!room.peer(alice).ui.state().video_share);
    assert!(
        !streams(&mut room, bob),
        "the end of the playlist must stop the finishing peer's share"
    );
}

/// A file request when the room shows nobody offering the file asks
/// nobody: the requester's player is given nothing to wait for chunks
/// that will never come.
#[tokio::test(start_paused = true)]
async fn a_file_request_without_a_provider_asks_nobody() {
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

    // nobody offers ep1
    room.peer(bob).act(ui::FileRequest {});
    assert!(
        room.hold(bob).is_empty(),
        "a request without a provider goes nowhere"
    );
    assert_eq!(room.peer(bob).player.state().source, None);
}

/// A stream the announced provider turns down — it is not actually serving
/// the file — fails, instead of leaving the viewer's player waiting for a
/// stream nobody will ever feed.
#[tokio::test(start_paused = true)]
async fn a_refused_stream_fails() {
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

    // alice's provider stops without the room noticing (her offer still
    // shows): bob's chunk requests still reach her and are turned down,
    // which must take his dead stream down with it
    room.peer(alice).model.video_provider.stop_providing();
    assert_eq!(offering(&mut room, bob), ["alice"]);
    room.peer(bob).act(ui::FileRequest {});
    assert_eq!(room.provider_of(bob), Some(alice));
    room.pump();
    assert_eq!(
        room.peer(bob).player.state().source,
        None,
        "a refused stream must not stay with the viewer's player"
    );
}

/// The users peer `who` sees offering the selected video.
fn offering(room: &mut Room, who: usize) -> Vec<String> {
    let ui = room.peer(who).ui.state();
    let mut names: Vec<String> = ui
        .users
        .iter()
        .filter(|u| {
            let offered = u.offers.as_ref().map(|f| f.name.as_str());
            offered.is_some() && offered == ui.playing_video.as_ref().map(|v| v.as_str())
        })
        .map(|u| u.name.to_string())
        .collect();
    names.sort();
    names
}

/// `who` shares the selected `video`, and its provider task reports the
/// file ready.
fn share(room: &mut Room, who: usize, video: &str) {
    room.peer(who).act(ui::FileShareChange {});
    room.peer(who).act(video_provider::FileReady {
        file_name: video.into(),
        size: 1_000,
    });
}

/// A room of alice hosting without the file, bob who has ep1 and carol
/// without it, with ep1 selected.
fn room_with_ep1_at_bob() -> (Room, usize, usize, usize) {
    let mut room = Room::new();
    let alice = room.join("alice", &[]);
    let bob = room.join("bob", &["ep1"]);
    let carol = room.join("carol", &[]);
    room.pump();
    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();
    (room, alice, bob, carol)
}

/// A user who offers the selected video shows so in everyone's user list,
/// that of a later joiner included, with the file's size. From that alone
/// the joiner streams the file from that user.
#[tokio::test(start_paused = true)]
async fn a_late_joiner_learns_who_offers_the_video() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    share(&mut room, bob, "ep1");
    room.pump();
    for who in [alice, bob, carol] {
        assert_eq!(offering(&mut room, who), ["bob"]);
    }

    let dave = room.join("dave", &[]);
    room.pump();
    assert_eq!(offering(&mut room, dave), ["bob"], "the greeting names it");
    let ep1 = SharedFile {
        name: "ep1".into(),
        size: FILE_SIZE,
    };
    let users = room.peer(dave).ui.state().users;
    let offer = users.iter().find_map(|u| u.offers.clone());
    assert_eq!(offer, Some(ep1.clone()), "the greeting carries the size");

    room.peer(dave).act(ui::FileRequest {});
    room.pump();
    let stream = Source::Stream {
        provider: room.peers[bob].id,
        file: ep1,
    };
    assert_eq!(room.peer(dave).player.state().source, Some(stream));
}

/// An offer leaves the room with its user: nobody sees it any more, the
/// stream from that user ends, and a new request finds no provider.
#[tokio::test(start_paused = true)]
async fn an_offer_leaves_with_its_user() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    share(&mut room, bob, "ep1");
    room.pump();
    room.peer(carol).act(ui::FileRequest {});
    room.pump();
    assert!(streams(&mut room, carol));

    room.leave(bob);
    room.pump();
    for who in [alice, carol] {
        assert!(offering(&mut room, who).is_empty(), "peer {who}");
    }
    assert!(!streams(&mut room, carol));

    room.peer(carol).act(ui::FileRequest {});
    assert!(room.hold(carol).is_empty(), "nobody is asked");
}

/// A new selection leaves no offer standing for it: an offer names its
/// video, so an offer of the old one, also one that was on its way to the
/// host while the selection changed, does not count for the new one.
#[tokio::test(start_paused = true)]
async fn a_new_selection_leaves_no_stale_offer() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    share(&mut room, bob, "ep1");
    room.pump();
    assert_eq!(offering(&mut room, carol), ["bob"]);

    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();
    for who in [alice, bob, carol] {
        assert!(offering(&mut room, who).is_empty(), "peer {who}");
    }

    // bob shares ep1 again while the host moves the room on to ep2: his
    // offer arrives after the selection
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();
    share(&mut room, bob, "ep1");
    let offer = room.hold(bob);
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.release(bob, offer);
    room.pump();
    let ep2 = Video::from("ep2");
    let host_users = room.peer(alice).model.state.users.clone();
    assert!(
        host_users.values().all(|u| u.offer_of(&ep2).is_none()),
        "an offer of ep1 must not stand for ep2"
    );
    for who in [alice, bob, carol] {
        assert!(offering(&mut room, who).is_empty(), "peer {who}");
    }
}

/// After the room moves to another host, the new host knows who offers the
/// video, and a request is answered by that user.
#[tokio::test(start_paused = true)]
async fn a_new_host_knows_who_offers_the_video() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    share(&mut room, bob, "ep1");
    room.pump();

    room.rehost(carol);
    room.pump();
    assert!(room.peer(carol).ui.state().is_host);
    for who in [alice, bob, carol] {
        assert_eq!(offering(&mut room, who), ["bob"], "peer {who}");
    }

    room.peer(alice).act(ui::FileRequest {});
    room.pump();
    assert_eq!(room.provider_of(alice), Some(bob));
}

/// What a user offers follows the share: shown once the file is open,
/// gone once the user stops sharing or the file cannot be read, and back
/// when the user shares again. A request after the share stopped is not
/// answered.
#[tokio::test(start_paused = true)]
async fn the_offer_follows_start_and_stop_of_sharing() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    let everyone_sees = |room: &mut Room, offering_users: &[&str]| {
        for who in [alice, bob, carol] {
            assert_eq!(offering(room, who), offering_users, "peer {who}");
        }
    };

    share(&mut room, bob, "ep1");
    room.pump();
    everyone_sees(&mut room, &["bob"]);

    // bob stops sharing
    room.peer(bob).act(ui::FileShareChange {});
    room.pump();
    assert!(!room.peer(bob).ui.state().video_share);
    everyone_sees(&mut room, &[]);
    room.peer(carol).act(ui::FileRequest {});
    assert!(room.hold(carol).is_empty(), "nobody is asked");

    // bob shares again, and then his file cannot be read
    share(&mut room, bob, "ep1");
    room.pump();
    everyone_sees(&mut room, &["bob"]);
    room.peer(bob).act(video_provider::SharingStopped);
    room.pump();
    everyone_sees(&mut room, &[]);
}

/// A stream is one of the selected video: a new selection ends it, and the
/// viewer's player reads nothing from the old provider anymore.
#[tokio::test(start_paused = true)]
async fn a_new_selection_ends_the_stream_of_the_old_video() {
    let (mut room, alice, bob, carol) = room_with_ep1_at_bob();
    share(&mut room, bob, "ep1");
    room.pump();
    room.peer(carol).act(ui::FileRequest {});
    room.pump();
    assert!(streams(&mut room, carol));

    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    room.pump();
    assert_eq!(room.peer(carol).player.state().source, None);
}

fn users(names: &[(&str, bool)]) -> Vec<(String, bool)> {
    names.iter().map(|(n, r)| (n.to_string(), *r)).collect()
}

/// Everyone, late joiners included, sees the same user list, and the host
/// keeps it current as users come, ready up and leave.
#[tokio::test(start_paused = true)]
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

/// A joiner whose name is taken gets a name of its own from the host, and
/// goes by it itself.
#[tokio::test(start_paused = true)]
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
#[tokio::test(start_paused = true)]
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
#[tokio::test(start_paused = true)]
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
        assert_eq!(peer.selected(), Some(Video::from("ep3")));
    }

    // a late joiner gets the same picture
    let charlie = room.join("charlie", &["ep1", "ep3"]);
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().video,
        Some(Video::from("ep3"))
    );
}

/// A room of alice (the host), bob and charlie, all playing ep1 at 10s.
async fn three_playing_ep1() -> (Room, usize, usize, usize) {
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
    start(&mut room, alice);
    room.pump();
    room.pass(10 * SECOND).await;
    room.heartbeat();
    room.pump();
    (room, alice, bob, charlie)
}

/// A user seeks in mpv, so the player is there before anyone else knows.
fn seek(room: &mut Room, who: usize, secs: u64) {
    let position = Duration::from_secs(secs);
    room.peer(who)
        .player
        .set(|player| player.position = position);
    room.peer(who).act(PlayerPositionChange::new(position));
}

/// The jumps of the room states that reached `who` since the last look,
/// which tell a state from before a seek from one after it.
fn jumps_seen(room: &mut Room, who: usize) -> Vec<u64> {
    let states = room.peer(who).take_states();
    states.iter().map(|state| state.jumps).collect()
}

/// A state the host published before it handled a seek moves nobody back:
/// not the seeker, whose intent it crossed, and not a peer it is still on
/// its way to, which gets the seek after it, never before.
#[tokio::test(start_paused = true)]
async fn a_state_older_than_a_seek_moves_nobody_back() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    room.hold_link(bob);
    room.hold_link(charlie);

    room.peer(alice).heartbeat();
    room.pump();
    seek(&mut room, bob, 300);
    room.release_link(bob);
    assert_eq!(
        room.peer(bob).player.state().position,
        Duration::from_secs(300),
        "a state that predates the seek must not undo it"
    );

    jumps_seen(&mut room, charlie);
    room.pump();
    assert_eq!(
        jumps_seen(&mut room, charlie),
        [],
        "the seek waits behind the state on its way"
    );
    room.release_link(charlie);
    let seen = jumps_seen(&mut room, charlie);
    assert_eq!(seen.len(), 2, "the old state, then the seek's: {seen:?}");
    assert!(seen[0] < seen[1], "the older state comes first: {seen:?}");
    for who in [alice, bob, charlie] {
        assert_eq!(
            room.peer(who).player.state().position,
            Duration::from_secs(300)
        );
    }

    // the room's clock counts as ever for what comes after the seek
    room.watch(20 * SECOND).await;
    drift(&mut room, charlie, 310);
    room.watch(SECOND).await;
    assert_eq!(position(&mut room, charlie), 321 * SECOND);
}

/// The host's player takes a moment to seek and reports the old position
/// meanwhile. The room is where it was told to go all the same: the
/// host's player follows the room, it does not say where the room is.
#[tokio::test(start_paused = true)]
async fn a_host_player_slow_to_seek_does_not_drag_the_room_back() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    room.peer(alice).player.set(|player| player.seek_lag = true);

    seek(&mut room, bob, 300);
    room.pump();
    room.heartbeat();
    room.pump();
    assert_eq!(
        room.peer(alice).player.state().position,
        Duration::from_secs(10),
        "the host's player is still on its way"
    );
    for who in [bob, charlie] {
        assert_eq!(
            room.peer(who).player.state().position,
            Duration::from_secs(300),
            "a host still seeking must not drag the room back"
        );
    }

    // a peer joining meanwhile starts at the seek's target
    let dave = room.join("dave", &["ep1"]);
    room.pump();
    assert_eq!(position(&mut room, dave), 300 * SECOND);

    // landed, the host's player plays along with the room
    room.peer(alice).player.land_seek();
    room.pass(10 * SECOND).await;
    room.heartbeat();
    room.pump();
    for who in [alice, bob, charlie, dave] {
        assert_eq!(position(&mut room, who), 310 * SECOND, "peer {who}");
    }
}

/// A player on its way to a position has none, so nothing is made of where
/// it is: it is left to land, and brought in line with where the room went
/// meanwhile once it has.
#[tokio::test(start_paused = true)]
async fn a_player_that_seeks_is_left_to_land() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    room.peer(bob).player.set(|player| player.seek_lag = true);

    // bob's player stalled, and is told to catch up
    let behind = position(&mut room, alice);
    room.watch(5 * SECOND).await;
    put(&mut room, bob, behind);
    room.watch(SECOND / 2).await;
    let told = position(&mut room, alice);

    // the room plays on and jumps while bob's player is on its way
    room.watch(5 * SECOND).await;
    seek(&mut room, charlie, 300);
    room.pump();
    room.watch(5 * SECOND).await;
    room.peer(bob).player.land_seek();
    assert_eq!(
        position(&mut room, bob),
        told,
        "a player on its way must not be told to seek again"
    );

    room.peer(bob).player.set(|player| player.seek_lag = false);
    room.watch(SECOND / 2).await;
    for who in [alice, bob, charlie] {
        let at = Duration::from_millis(305_500);
        assert_eq!(position(&mut room, who), at, "peer {who}");
    }
}

/// A state the host published before it handled a client's pause does not
/// start the client again on its way.
#[tokio::test(start_paused = true)]
async fn a_state_from_before_the_host_handled_a_pause_does_not_undo_it() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    room.hold_link(bob);

    room.peer(alice).heartbeat();
    room.pump();
    room.peer(bob).player.set(|player| player.paused = true);
    room.peer(bob).act(PlayerPause);
    room.release_link(bob);
    assert!(
        room.peer(bob).player.state().paused,
        "a state that predates the pause must not start bob again"
    );

    room.pump();
    for who in [alice, bob, charlie] {
        assert!(room.peer(who).player.state().paused, "pause must propagate");
    }
}

/// A client whose session ended without the host noticing joins again, and
/// the host still knows it from before. The host counts what it handled of
/// the client anew, so a state from before the client's next seek does not
/// pass for its outcome.
#[tokio::test(start_paused = true)]
async fn a_client_the_host_still_knows_counts_its_intents_anew() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    seek(&mut room, bob, 100);
    seek(&mut room, bob, 200);
    room.pump();

    let reason = "the session ended".to_string();
    room.deliver(bob, DisconnectedMsg { reason });
    room.reconnect(bob);
    room.pump();
    assert_eq!(position(&mut room, bob), 200 * SECOND, "back with the room");

    room.hold_link(bob);
    room.peer(alice).heartbeat();
    room.pump();
    seek(&mut room, bob, 300);
    room.release_link(bob);
    assert_eq!(
        position(&mut room, bob),
        300 * SECOND,
        "a state that predates the seek must not undo it"
    );

    room.pump();
    for who in [alice, bob, charlie] {
        assert_eq!(position(&mut room, who), 300 * SECOND, "peer {who}");
    }
}

/// A seek shorter than the tolerance moves every peer by exactly that much:
/// a jump is set, not judged by the offset it leaves.
#[tokio::test(start_paused = true)]
async fn a_seek_within_the_tolerance_reaches_every_peer() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;

    let to = Duration::from_millis(10_300);
    put(&mut room, bob, to);
    room.peer(bob).act(PlayerPositionChange::new(to));
    room.pump();
    for who in [alice, bob, charlie] {
        assert_eq!(
            room.peer(who).player.state().position,
            to,
            "a client's 0.3s seek must reach everyone"
        );
    }

    let to = Duration::from_millis(10_600);
    put(&mut room, alice, to);
    room.peer(alice).act(PlayerPositionChange::new(to));
    room.pump();
    for who in [alice, bob, charlie] {
        assert_eq!(
            room.peer(who).player.state().position,
            to,
            "the host's 0.3s seek must reach everyone"
        );
    }
}

/// Two clients seeking at the same moment end where the host's order puts
/// them: the room takes the one the host handled last. The state between
/// the two, which shows only the first seek, does not move the second
/// seeker's player there and back.
#[tokio::test(start_paused = true)]
async fn two_clients_acting_at_once_end_in_the_hosts_order() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;

    // both before either reached the host; bob's arrives first
    seek(&mut room, bob, 200);
    seek(&mut room, charlie, 300);
    let bobs = room.hold(bob);
    let charlies = room.hold(charlie);
    room.release(bob, bobs);
    room.pump();
    assert_eq!(position(&mut room, alice), 200 * SECOND);
    assert_eq!(
        position(&mut room, charlie),
        300 * SECOND,
        "a state from before the host handled charlie's seek must not undo it"
    );
    room.release(charlie, charlies);
    room.pump();

    for who in [alice, bob, charlie] {
        assert_eq!(
            room.peer(who).player.state().position,
            Duration::from_secs(300),
            "peer {who} must end with the room on the last seek the host handled"
        );
    }
}

/// The host's position is half a round trip old when it arrives; a playing
/// client is judged against where the host is by now, a paused one against
/// the reported position as is.
#[tokio::test(start_paused = true)]
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

    start(&mut room, alice);
    room.pump();
    room.watch(100 * SECOND).await;
    drift(&mut room, bob, 90);
    room.watch(SECOND).await;
    assert_eq!(
        position(&mut room, bob),
        Duration::from_millis(101_200),
        "a playing room is half the quietest round trip further by now"
    );

    // 0.6s ahead of the reported position, but only 0.4s ahead of where
    // the room is by now: within the tolerance
    put(&mut room, bob, Duration::from_millis(101_600));
    room.watch(5 * SECOND).await;
    assert_eq!(position(&mut room, bob), Duration::from_millis(106_600));

    // a paused room does not move while the message travels
    pause(&mut room, alice);
    room.pump();
    drift(&mut room, bob, 90);
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(position(&mut room, bob), 106 * SECOND);
}

/// A room moving to another host keeps its state: the new host does not
/// restart the running video, and everyone who reconnects to it — the old
/// host included — is greeted with the playlist, the video at the new
/// host's position, and the full user list.
#[tokio::test(start_paused = true)]
async fn a_new_host_keeps_the_room_running() {
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
    start(&mut room, alice);
    room.pump();
    room.pass(60 * SECOND).await;

    room.rehost(bob);
    room.pump();

    assert_eq!(room.host_index(), bob);
    let bob_player = room.peer(bob).player.state();
    assert_eq!(bob_player.video, Some(Video::from("ep2")));
    assert_eq!(
        bob_player.position,
        60 * SECOND,
        "the new host must not restart the running video"
    );
    // alice started the room, which made her ready
    let everyone = users(&[("alice", true), ("bob", false), ("charlie", false)]);
    for who in [alice, bob, charlie] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep1", "ep2"]));
        let player = peer.player.state();
        assert_eq!(player.video, Some(Video::from("ep2")));
        assert_eq!(player.position, 60 * SECOND);
        assert!(!player.paused, "peer {who}: the room plays on");
        assert_eq!(peer.ui.state().user_names(), everyone);
    }
}

/// The playlist of an earlier session: ep1 and ep2, with ep2 playing.
fn saved_playlist(_: &RoomName) -> Option<PlaylistHandler> {
    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    Some(PlaylistHandler::new(playlist, Some(&Video::from("ep2"))))
}

/// A host starting a room fresh restores the room's saved playlist and
/// selection, and peers joining later are greeted with it.
#[tokio::test(start_paused = true)]
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
#[tokio::test(start_paused = true)]
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

    room.rehost(bob);
    room.pump();

    assert_eq!(room.host_index(), bob);
    for who in [alice, bob] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), Playlist::from_iter(["ep3"]));
        assert_eq!(peer.player.state().video, Some(Video::from("ep3")));
    }
}

/// A host restoring a saved room tells its own chat what it restored, in
/// its own user's name. A host without a saved room restores nothing and
/// says nothing.
#[tokio::test(start_paused = true)]
async fn a_fresh_host_tells_its_chat_what_it_restored() {
    let mut room = Room::with_saved_playlist(saved_playlist);
    let alice = room.join("alice", &["ep1", "ep2"]);
    room.pump();
    assert_eq!(
        room.peer(alice).ui.state().actions,
        ["alice changed playlist", "alice selected ep2"]
    );

    let mut room = Room::new();
    let alice = room.join("alice", &["ep1", "ep2"]);
    room.pump();
    assert!(room.peer(alice).ui.state().actions.is_empty());
}

/// Every peer sees the line for an action once, its actor included, be it
/// the host's own user or a client's. The states that follow add none.
#[tokio::test(start_paused = true)]
async fn every_peer_sees_an_action_line_once_the_actor_included() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    let charlie = room.join("charlie", &["ep1"]);
    room.pump();
    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(bob).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    room.pump();
    let expected = ["alice changed playlist", "bob selected ep1"];
    for who in [alice, bob, charlie] {
        assert_eq!(room.peer(who).ui.state().actions, expected);
    }

    room.peer(bob).player.set(|player| player.paused = false);
    room.peer(bob).act(PlayerStart);
    room.pump();
    room.peer(alice).act(PlayerSpeedChange::new(1.5));
    room.pump();
    for _ in 0..3 {
        room.heartbeat();
        room.pump();
    }

    let expected = [
        "alice changed playlist",
        "bob selected ep1",
        "bob started playback",
        "alice changed playback speed to 1.5",
    ];
    for who in [alice, bob, charlie] {
        assert_eq!(room.peer(who).ui.state().actions, expected);
    }
}

/// A client whose selection has not reached the host shows another video
/// than the room, while its player stays with the room. When the request
/// fails, the client's session ends, and the client shows the room's video
/// again.
#[tokio::test(start_paused = true)]
async fn a_selection_the_host_never_got_gives_way_to_the_rooms() {
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

    // bob selects ep1, which his UI shows at once, and the request stays
    // on its way to the host
    let ep1 = Some(Video::from("ep1"));
    room.peer(bob).ui.set(|ui| ui.playing_video = ep1.clone());
    room.peer(bob).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    let _on_its_way = room.hold(bob);

    for _ in 0..10 {
        room.heartbeat();
        room.pump();
    }
    assert_eq!(
        room.peer(bob).ui.state().playing_video,
        ep1,
        "every state so far predates the host's answer"
    );
    assert_eq!(
        room.peer(bob).player.state().video,
        Some(Video::from("ep2")),
        "the player waits for the room"
    );

    room.fail_link(bob);
    room.pump();
    let player = room.peer(bob).player.state();
    assert_eq!(player.video, Some(Video::from("ep2")));
    assert_eq!(room.peer(bob).selected(), Some(Video::from("ep2")));
    assert_eq!(
        room.peer(bob).ui.state().playing_video,
        Some(Video::from("ep2"))
    );
}

/// A seek can name another video than the one the host plays, when it
/// crossed a selection on the way. The host moves the room to it, and
/// greets a peer joining afterwards with the video the room plays.
#[tokio::test(start_paused = true)]
async fn a_seek_into_another_video_is_what_a_late_joiner_is_greeted_with() {
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

    let peer = room.peer(bob).id;
    let intent = Intent::Seek {
        video: Video::from("ep2"),
        position: Duration::from_secs(300),
    };
    room.deliver(alice, PeerIntentMsg { peer, intent });
    room.pump();
    for who in [alice, bob] {
        let player = room.peer(who).player.state();
        assert_eq!(player.video, Some(Video::from("ep2")));
        assert_eq!(player.position, Duration::from_secs(300));
    }

    let charlie = room.join("charlie", &["ep1", "ep2"]);
    room.pump();
    assert_eq!(
        room.peer(charlie).player.state().video,
        Some(Video::from("ep2")),
        "a late joiner must load the video the host plays"
    );
}

/// The playlists the room states in `states` carry.
fn playlists(states: &[RoomStateMsg]) -> Vec<Option<Playlist>> {
    states.iter().map(|state| state.playlist.clone()).collect()
}

/// A new playlist goes to every member with the first state after its
/// change; the states after it leave it out for a member that took it, and
/// every member has it.
#[tokio::test(start_paused = true)]
async fn a_playlist_reaches_each_member_once() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    let charlie = room.join("charlie", &["ep1"]);
    room.pump();
    for who in [bob, charlie] {
        room.peer(who).take_states();
    }

    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();
    for _ in 0..3 {
        room.heartbeat();
        room.pump();
    }
    for who in [bob, charlie] {
        let carried = playlists(&room.peer(who).take_states());
        assert_eq!(carried.len(), 4, "the change and three heartbeats");
        assert_eq!(carried[0], Some(playlist.clone()), "the change carries it");
        assert_eq!(carried[1..], [None, None, None], "a heartbeat does not");
    }
    for who in [alice, bob, charlie] {
        assert_eq!(room.peer(who).playlist(), playlist);
        assert_eq!(room.peer(who).ui.state().playlist, playlist);
    }
}

/// A message the host cannot write to a member's stream ends the stream
/// and drops the member from the room. The member connects again and is greeted with what it missed,
/// the playlist included.
#[tokio::test(start_paused = true)]
async fn a_member_whose_message_failed_rejoins_with_the_rooms_playlist() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();

    room.hold_link(bob);
    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();
    assert_eq!(room.peer(bob).playlist(), Playlist::default());

    room.fail_link(bob);
    room.pump();
    assert_eq!(room.peer(bob).playlist(), playlist);
    assert_eq!(room.peer(bob).ui.state().playlist, playlist);
    assert_eq!(
        room.peer(alice).ui.state().user_names(),
        users(&[("alice", false), ("bob", false)]),
        "bob is back in the room"
    );
}

/// What the host sends a member while a message to it is on its way waits
/// behind it: a heartbeat after a playlist change reaches the member after
/// the change, never before. Waiting, the heartbeat takes the place of the
/// change's state, and carries the playlist the member does not have yet.
#[tokio::test(start_paused = true)]
async fn a_heartbeat_waits_behind_a_playlist_change() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();
    room.peer(bob).take_states();

    room.hold_link(bob);
    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();
    room.peer(alice).heartbeat();
    room.pump();
    assert_eq!(room.peer(bob).playlist(), Playlist::default());
    assert!(room.peer(bob).ui.state().actions.is_empty());

    room.release_link(bob);
    assert_eq!(
        room.peer(bob).ui.state().actions,
        ["alice changed playlist"],
        "the action line was on its way first"
    );
    let states = room.peer(bob).take_states();
    assert_eq!(playlists(&states), [Some(playlist.clone())]);
    assert_eq!(room.peer(bob).playlist(), playlist);

    room.heartbeat();
    room.pump();
    assert_eq!(playlists(&room.peer(bob).take_states()), [None]);
}

/// A joiner takes the room's playlist from the first state the host sends
/// it; the states after it leave the playlist out.
#[tokio::test(start_paused = true)]
async fn a_joiner_gets_the_playlist_from_the_greeting() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    room.pump();
    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();

    let charlie = room.join("charlie", &["ep1"]);
    room.pump();
    room.heartbeat();
    room.pump();
    let carried = playlists(&room.peer(charlie).take_states());
    assert_eq!(
        carried[0],
        Some(playlist.clone()),
        "the greeting carries it"
    );
    assert!(carried[1..].iter().all(Option::is_none), "{carried:?}");
    assert_eq!(room.peer(charlie).playlist(), playlist);
    assert_eq!(room.peer(charlie).ui.state().playlist, playlist);
}

/// A new playlist that reaches a client while intents of its own are on
/// their way to the host comes with a state from before them, which the
/// client ignores. The client keeps the playlist and shows it with the
/// host's answer.
#[tokio::test(start_paused = true)]
async fn a_playlist_change_while_intents_are_pending_still_reaches_the_client() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;

    room.peer(bob).player.set(|player| player.paused = true);
    room.peer(bob).act(PlayerPause);
    seek(&mut room, bob, 300);
    let intents = room.hold(bob);

    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();
    assert_eq!(room.peer(charlie).playlist(), playlist);
    assert_eq!(
        room.peer(bob).playlist(),
        Playlist::from_iter(["ep1"]),
        "the playlist came before the host's answer"
    );

    room.release(bob, intents);
    room.pump();
    for who in [alice, bob, charlie] {
        let peer = room.peer(who);
        assert_eq!(peer.playlist(), playlist);
        assert_eq!(peer.ui.state().playlist, playlist);
        assert!(peer.player.state().paused);
        assert_eq!(peer.player.state().position, Duration::from_secs(300));
    }
}

/// A client's playlist edit that has not reached the host shadows the
/// room's while the host may still handle its intent. When the client's
/// stream ends instead, the client's session ends, and the client shows the room's playlist
/// again.
#[tokio::test(start_paused = true)]
async fn a_playlist_edit_the_host_never_got_gives_way_to_the_rooms() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &["ep1"]);
    room.pump();
    let playlist = Playlist::from_iter(["ep1", "ep2"]);
    room.peer(alice).act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    room.pump();

    let edit = Playlist::from_iter(["ep2"]);
    room.peer(bob).ui.set(|ui| ui.playlist = edit.clone());
    room.peer(bob).act(ui::PlaylistChange {
        playlist: edit.clone(),
    });
    let _on_its_way = room.hold(bob);
    for _ in 0..10 {
        room.heartbeat();
        room.pump();
    }
    assert_eq!(room.peer(bob).ui.state().playlist, edit);
    assert_eq!(room.peer(bob).playlist(), playlist, "the room's is kept");

    room.fail_link(bob);
    room.pump();
    assert_eq!(room.peer(bob).playlist(), playlist);
    assert_eq!(room.peer(bob).ui.state().playlist, playlist);
}

/// What one client asks of the room reaches the host in the order the
/// client asked it, also when it arrives all at once: the room ends where
/// the client's last intent puts it, and every chat shows the client's
/// actions in its order.
#[tokio::test(start_paused = true)]
async fn one_clients_intents_reach_the_host_in_the_order_sent() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;
    let before = room.peer(alice).ui.state().actions.len();

    seek(&mut room, bob, 100);
    seek(&mut room, bob, 200);
    room.peer(bob).player.set(|player| player.paused = true);
    room.peer(bob).act(PlayerPause);
    let held = room.hold(bob);
    room.release(bob, held);
    room.pump();

    for who in [alice, bob, charlie] {
        let peer = room.peer(who);
        assert_eq!(peer.player.state().position, Duration::from_secs(200));
        assert!(peer.player.state().paused);
        assert_eq!(
            peer.ui.state().actions[before..],
            [
                "bob seeked to 100s",
                "bob seeked to 200s",
                "bob paused playback"
            ],
            "peer {who}"
        );
    }
}

/// A client's message the host never takes ends the client's session,
/// and what the client asked goes with it. The client connects again and
/// follows the room as the host has it.
#[tokio::test(start_paused = true)]
async fn a_client_whose_message_failed_rejoins_and_follows_the_room() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;

    room.peer(bob).player.set(|player| player.paused = true);
    room.peer(bob).act(PlayerPause);
    room.fail_link(bob);
    room.pump();

    for who in [alice, bob, charlie] {
        let peer = room.peer(who);
        assert!(!peer.player.state().paused, "peer {who}: the room plays on");
        assert_eq!(peer.player.state().video, Some(Video::from("ep1")));
    }
    assert_eq!(
        room.peer(alice).ui.state().user_names(),
        users(&[("alice", true), ("bob", false), ("charlie", false)]),
        "bob is back in the room"
    );
}

/// A follower whose session ends is in no room and the authority over the
/// state it has: its player plays on, and what its user does applies at
/// once. Back in the room, it takes the room's state again.
#[tokio::test(start_paused = true)]
async fn a_follower_without_its_session_keeps_playing_and_rejoins_the_room() {
    let (mut room, alice, bob, charlie) = three_playing_ep1().await;

    room.lose_connection(bob);
    room.pump();
    room.pass(5 * SECOND).await;
    room.heartbeat();
    room.pump();
    let player = room.peer(bob).player.state();
    assert!(!player.paused, "bob plays on by himself");
    assert_eq!(player.position, 15 * SECOND);
    assert_eq!(
        room.peer(bob).ui.state().user_names(),
        users(&[("bob", false)]),
        "nobody else is in a room of one"
    );

    // on his own, bob decides: his seek and pause are his state's
    seek(&mut room, bob, 500);
    pause(&mut room, bob);
    room.pass(5 * SECOND).await;
    room.heartbeat();
    room.pump();
    assert_eq!(position(&mut room, bob), 500 * SECOND);
    for who in [alice, charlie] {
        assert_eq!(position(&mut room, who), 20 * SECOND, "peer {who}");
    }

    // back in the room, the room's state overwrites what bob did alone
    room.reconnect(bob);
    room.pump();
    let player = room.peer(bob).player.state();
    assert!(!player.paused);
    assert_eq!(player.position, 20 * SECOND);
    assert_eq!(
        room.peer(bob).ui.state().user_names(),
        users(&[("alice", true), ("bob", false), ("charlie", false)])
    );
}

/// A peer that does not have the selected video plays nothing. Its player
/// loads the video where the room is once the core has something to play
/// it from: the stream of a peer that offers it, or the file when the file
/// database finds it. A file found while the stream plays leaves the stream
/// alone.
#[tokio::test(start_paused = true)]
async fn a_peer_without_the_file_plays_it_once_it_has_a_source() {
    let mut room = Room::new();
    let alice = room.join("alice", &["ep1"]);
    let bob = room.join("bob", &[]);
    let carol = room.join("carol", &[]);
    room.pump();
    room.peer(alice).act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1"]),
    });
    room.peer(alice).act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    seek(&mut room, alice, 40);
    room.pump();
    for who in [bob, carol] {
        assert_eq!(room.peer(who).selected(), Some(Video::from("ep1")));
        assert_eq!(room.peer(who).player.state().source, None, "peer {who}");
    }

    share(&mut room, alice, "ep1");
    room.pump();
    room.peer(bob).act(ui::FileRequest {});
    room.pump();
    let stream = Some(Source::Stream {
        provider: room.peers[alice].id,
        file: SharedFile {
            name: "ep1".into(),
            size: FILE_SIZE,
        },
    });
    let player = room.peer(bob).player.state();
    assert_eq!(player.source, stream);
    assert_eq!(player.position, 40 * SECOND);
    room.peer(bob).find_file("ep1");
    assert_eq!(room.peer(bob).player.state().source, stream);

    room.peer(carol).find_file("ep2");
    assert_eq!(room.peer(carol).player.state().source, None);
    room.peer(carol).find_file("ep1");
    let player = room.peer(carol).player.state();
    assert_eq!(player.source, Some(Source::File("/ep1".into())));
    assert_eq!(player.position, 40 * SECOND);
}

/// A core in no room is a room of one: its user selects, plays, seeks and
/// reaches the end of a video without anyone to ask.
#[tokio::test(start_paused = true)]
async fn a_core_in_no_room_selects_plays_and_seeks_locally() {
    let mut alice = Peer::in_no_room("alice", &["ep1", "ep2"]);
    alice.act(ui::PlaylistChange {
        playlist: Playlist::from_iter(["ep1", "ep2"]),
    });
    alice.act(ui::VideoChange {
        video: Video::from("ep1"),
    });
    alice.hear_player();
    let player = alice.player.state();
    assert_eq!(player.video, Some(Video::from("ep1")));
    assert!(player.paused, "a new selection waits for its user");
    assert_eq!(alice.ui.state().playing_video, Some(Video::from("ep1")));

    alice.player.set(|player| player.paused = false);
    alice.act(PlayerStart);
    tokio::time::advance(10 * SECOND).await;
    alice.heartbeat();
    assert_eq!(alice.player.state().position, 10 * SECOND);

    alice.player.set(|player| player.position = 300 * SECOND);
    alice.act(PlayerPositionChange::new(300 * SECOND));
    tokio::time::advance(10 * SECOND).await;
    alice.heartbeat();
    assert_eq!(alice.player.state().position, 310 * SECOND);

    // ready and alone, the next video starts by itself once it has loaded
    alice.reach_end();
    alice.hear_player();
    let player = alice.player.state();
    assert_eq!(player.video, Some(Video::from("ep2")));
    assert_eq!(player.position, Duration::ZERO);
    assert!(!player.paused);
    assert_eq!(
        alice.ui.state().actions,
        [
            "alice changed playlist",
            "alice selected ep1",
            "alice started playback",
            "alice seeked to 300s",
            "alice selected ep2",
            "alice started playback",
        ]
    );
}
