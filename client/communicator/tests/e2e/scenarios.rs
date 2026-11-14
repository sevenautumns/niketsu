//! Scenarios. Each test name is one sentence of what the transport carries.

use std::time::Duration;

use niketsu_core::file_database::FileEntry;
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::ui;

use crate::harness::{Peer, Relay, download, eventually};

fn select(peer: &Peer, playlist: &[&str], video: &str) {
    peer.ui.act(ui::PlaylistChange {
        playlist: Playlist::from_iter(playlist.iter().copied()),
    });
    peer.ui.act(ui::VideoChange {
        video: Video::from(video),
    });
}

/// The first peer at the relay hosts the room, the second one joins it, and
/// both end up with the same user list.
#[tokio::test(flavor = "multi_thread")]
async fn two_peers_meet_in_a_room() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join(&relay, "bob", &[]);

    eventually("bob joins as a client", || {
        bob.ui.state().is_host == Some(false)
    })
    .await;
    for (name, peer) in [("alice", &alice), ("bob", &bob)] {
        eventually(&format!("{name} sees both users"), || {
            peer.ui.state().user_names() == ["alice", "bob"]
        })
        .await;
    }
}

/// A selection, a seek and a pause by one peer reach the other, in both
/// directions: the host's travel as gossip, the client's as well.
#[tokio::test(flavor = "multi_thread")]
async fn playback_control_reaches_the_other_peer() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &["ep1", "ep2"]).hosting().await;
    let bob = Peer::join(&relay, "bob", &["ep1", "ep2"]);
    eventually("alice sees bob", || {
        alice.ui.state().user_names() == ["alice", "bob"]
    })
    .await;

    select(&alice, &["ep1", "ep2"], "ep1");
    eventually("bob loads alice's selection", || {
        bob.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    eventually("bob has alice's playlist", || {
        bob.ui.state().playlist == Playlist::from_iter(["ep1", "ep2"])
    })
    .await;

    alice.player.start();
    eventually("bob plays", || !bob.player.state().paused).await;

    bob.player.seek(Duration::from_secs(300));
    eventually("alice follows bob's seek", || {
        alice.player.state().position == Duration::from_secs(300)
    })
    .await;

    bob.player.pause();
    eventually("alice pauses with bob", || alice.player.state().paused).await;
}

/// Whoever joins a running session is brought to the host's playlist, video
/// and position.
#[tokio::test(flavor = "multi_thread")]
async fn late_joiner_catches_up_with_the_host() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &["ep1"]).hosting().await;
    select(&alice, &["ep1"], "ep1");
    eventually("alice has the video loaded", || {
        alice.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    alice.player.seek(Duration::from_secs(600));

    let bob = Peer::join(&relay, "bob", &["ep1"]);
    eventually("bob is where alice is", || {
        let player = bob.player.state();
        player.video == Some(Video::from("ep1")) && player.position == Duration::from_secs(600)
    })
    .await;
    eventually("bob has alice's playlist", || {
        bob.ui.state().playlist == Playlist::from_iter(["ep1"])
    })
    .await;
}

/// A handover moves the room at the relay: the chosen peer comes back as
/// the host, the old host as its client, and the room keeps working.
#[tokio::test(flavor = "multi_thread")]
async fn handover_swaps_the_roles() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &["ep1"]).hosting().await;
    let bob = Peer::join(&relay, "bob", &["ep1"]);
    eventually("alice sees bob", || {
        alice.ui.state().user_names() == ["alice", "bob"]
    })
    .await;

    alice.ui.act(ui::HostHandover {
        new_host: "bob".into(),
    });
    eventually("bob hosts and alice is its client", || {
        bob.ui.state().is_host == Some(true) && alice.ui.state().is_host == Some(false)
    })
    .await;
    for (name, peer) in [("alice", &alice), ("bob", &bob)] {
        eventually(&format!("{name} sees both users again"), || {
            peer.ui.state().user_names() == ["alice", "bob"]
        })
        .await;
    }

    select(&bob, &["ep1"], "ep1");
    eventually("alice loads the new host's selection", || {
        alice.player.state().video == Some(Video::from("ep1"))
    })
    .await;
}

/// A peer without the file plays it from the peer that shares it: what its
/// video server hands the player is the shared file, byte for byte.
#[tokio::test(flavor = "multi_thread")]
async fn shared_file_streams_to_a_peer_without_it() {
    let ep1 = Episode::on_disk();
    let relay = Relay::start();
    let alice = Peer::join_with_files(&relay, "alice", ep1.files())
        .hosting()
        .await;
    let bob = Peer::join(&relay, "bob", &[]);
    eventually("alice sees bob", || {
        alice.ui.state().user_names() == ["alice", "bob"]
    })
    .await;

    select(&alice, &["ep1"], "ep1");
    ep1.stream(&alice, &bob).await;
}

/// Two clients share a file without the host having it: the host only
/// bridges their hole punch, the bytes travel between the two directly.
#[tokio::test(flavor = "multi_thread")]
async fn shared_file_streams_between_two_clients() {
    let ep1 = Episode::on_disk();
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join_with_files(&relay, "bob", ep1.files());
    let carol = Peer::join(&relay, "carol", &[]);
    eventually("alice sees everyone", || {
        alice.ui.state().user_names() == ["alice", "bob", "carol"]
    })
    .await;

    select(&alice, &["ep1"], "ep1");
    eventually("bob has the video selected", || {
        bob.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    ep1.stream(&bob, &carol).await;
}

/// When the peer that shares a file leaves, the peer playing it is told so
/// once and its video server stops, instead of asking for chunks nobody
/// will send.
#[tokio::test(flavor = "multi_thread")]
async fn stream_ends_when_its_provider_leaves() {
    let ep1 = Episode::on_disk();
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join_with_files(&relay, "bob", ep1.files());
    let carol = Peer::join(&relay, "carol", &[]);
    eventually("alice sees everyone", || {
        alice.ui.state().user_names() == ["alice", "bob", "carol"]
    })
    .await;

    select(&alice, &["ep1"], "ep1");
    eventually("bob has the video selected", || {
        bob.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    let source = ep1.open(&bob, &carol).await;
    let video_server = source.strip_prefix("http://").unwrap().to_string();
    let told = || {
        let left = |line: &&String| line.contains("provider of the file disconnected");
        carol.ui.state().messages.iter().filter(left).count()
    };

    assert_eq!(told(), 0);

    drop(bob);
    eventually("carol hears that the provider left", || told() == 1).await;
    eventually("carol's video server is gone", || {
        std::net::TcpStream::connect(&video_server).is_err()
    })
    .await;
    assert_eq!(told(), 1);
}

/// The video "ep1" as a file on disk: several chunks long, the last of them
/// not a full one.
struct Episode {
    dir: tempfile::TempDir,
    content: Vec<u8>,
}

impl Episode {
    fn on_disk() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let content: Vec<u8> = (0..1_300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(dir.path().join("ep1"), &content).unwrap();
        Self { dir, content }
    }

    /// The file database of a peer that has the episode.
    fn files(&self) -> Vec<FileEntry> {
        vec![FileEntry::new(
            "ep1".into(),
            self.dir.path().join("ep1"),
            None,
        )]
    }

    /// `provider` shares the selected episode, `viewer` requests it and
    /// reads it from its video server like its player would.
    async fn stream(&self, provider: &Peer, viewer: &Peer) {
        let source = self.open(provider, viewer).await;
        let received = download(&source).await;
        assert_eq!(received.len(), self.content.len());
        assert!(received == self.content, "the stream differs from the file");
    }

    /// `provider` shares the selected episode and `viewer` requests it.
    /// Returns where the viewer's player reads it from: the URL of its
    /// video server.
    async fn open(&self, provider: &Peer, viewer: &Peer) -> String {
        provider.ui.act(ui::FileShareChange {});
        eventually("the provider shares and the viewer has it selected", || {
            provider.ui.state().video_share
                && viewer.player.state().video == Some(Video::from("ep1"))
        })
        .await;

        viewer.ui.act(ui::FileRequest {});
        eventually("the viewer's player is pointed at its video server", || {
            viewer.player.state().source.is_some()
        })
        .await;
        viewer.player.state().source.unwrap()
    }
}
