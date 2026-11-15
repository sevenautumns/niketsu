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

/// A peer with the wrong password is told so once and stays out of the
/// room instead of asking the relay again.
#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_password_keeps_a_peer_out() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let mallory = Peer::join_with_password(&relay, "mallory", "guess");

    let denials = || {
        mallory
            .ui
            .state()
            .messages
            .iter()
            .filter(|m| m.contains("Wrong password"))
            .count()
    };
    eventually("mallory is told of the wrong password", || denials() > 0).await;
    // The one wait for something not to happen: a retry would come within
    // a few reconnect intervals.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(denials(), 1, "mallory asked the relay again");
    assert_eq!(mallory.ui.state().is_host, None);
    assert_eq!(alice.ui.state().user_names(), ["alice"]);
}

/// A selection, a seek and a pause by one peer reach the other, in both
/// directions: the host's travel in its states, the client's as requests
/// to the host.
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

    // the room plays on from the seek, and the players here stand still:
    // alice is put where the room is, at the seek or shortly after it
    bob.player.seek(Duration::from_secs(300));
    eventually("alice follows bob's seek", || {
        let position = alice.player.state().position;
        (Duration::from_secs(300)..Duration::from_secs(360)).contains(&position)
    })
    .await;

    bob.player.pause();
    eventually("alice pauses with bob", || alice.player.state().paused).await;
}

/// Alice hosts, bob and carol are her clients, each seeing all three.
async fn three_in_a_room(relay: &Relay) -> [Peer; 3] {
    let alice = Peer::join(relay, "alice", &[]).hosting().await;
    let bob = Peer::join(relay, "bob", &[]);
    let carol = Peer::join(relay, "carol", &[]);
    for peer in [&alice, &bob, &carol] {
        eventually(&format!("{} sees everyone", peer.name()), || {
            peer.ui.state().user_names() == ["alice", "bob", "carol"]
        })
        .await;
    }
    [alice, bob, carol]
}

/// Every peer shows `line` exactly once, its author included.
async fn everyone_shows_once(peers: &[Peer], line: &str) {
    let count = |peer: &Peer| {
        peer.ui
            .state()
            .messages
            .iter()
            .filter(|m| *m == line)
            .count()
    };
    for peer in peers {
        eventually(&format!("{} shows {line:?}", peer.name()), || {
            count(peer) > 0
        })
        .await;
    }
    // The one other wait for something not to happen: a second copy would
    // be on its way already.
    tokio::time::sleep(Duration::from_secs(1)).await;
    for peer in peers {
        assert_eq!(
            count(peer),
            1,
            "{} shows {line:?} more than once",
            peer.name()
        );
    }
}

/// A client's chat line reaches the host and the other client once each,
/// and its author shows it once, from the moment it was sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_clients_chat_line_reaches_everyone_once() {
    let relay = Relay::start();
    let peers = three_in_a_room(&relay).await;
    peers[1].ui.say("hello from bob");
    everyone_shows_once(&peers, "hello from bob").await;
}

/// The host's chat line reaches every client once.
#[tokio::test(flavor = "multi_thread")]
async fn the_hosts_chat_line_reaches_everyone_once() {
    let relay = Relay::start();
    let peers = three_in_a_room(&relay).await;
    peers[0].ui.say("hello from alice");
    everyone_shows_once(&peers, "hello from alice").await;
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

/// A user who stops sharing no longer shows as offering the video, for
/// anyone, and a request after that asks nobody.
#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_share_withdraws_the_offer_for_everyone() {
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

    bob.ui.act(ui::FileShareChange {});
    for peer in [&alice, &bob, &carol] {
        eventually(&format!("{} sees bob offering", peer.name()), || {
            peer.ui.state().offering() == ["bob"]
        })
        .await;
    }
    bob.ui.act(ui::FileShareChange {});
    for peer in [&alice, &bob, &carol] {
        eventually(&format!("{} sees no offer", peer.name()), || {
            peer.ui.state().offering().is_empty()
        })
        .await;
    }
    assert!(!bob.ui.state().video_share);

    carol.ui.act(ui::FileRequest {});
    eventually("carol is told nobody offers it", || {
        let no_provider = |m: &String| m.contains("No providers found");
        carol.ui.state().messages.iter().any(no_provider)
    })
    .await;
    assert_eq!(carol.player.state().source, None);
}

/// A new selection ends the stream of the old video: the viewer's video
/// server stops, and the viewer hears nothing more of the old provider,
/// also when that provider leaves.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_selection_ends_the_stream_quietly() {
    let ep1 = Episode::on_disk();
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join_with_files(&relay, "bob", ep1.files());
    let carol = Peer::join(&relay, "carol", &[]);
    eventually("alice sees everyone", || {
        alice.ui.state().user_names() == ["alice", "bob", "carol"]
    })
    .await;
    select(&alice, &["ep1", "ep2"], "ep1");
    eventually("bob has the video selected", || {
        bob.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    // carol plays the stream: her player reads it, over a direct link to bob
    let source = ep1.open(&bob, &carol).await;
    assert!(download(&source).await == ep1.content);
    let video_server = source.strip_prefix("http://").unwrap().to_string();

    alice.ui.act(ui::VideoChange {
        video: Video::from("ep2"),
    });
    eventually("carol's video server is gone", || {
        std::net::TcpStream::connect(&video_server).is_err()
    })
    .await;
    eventually("bob's offer went with the selection", || {
        carol.ui.state().offering().is_empty()
    })
    .await;

    drop(bob);
    eventually("bob leaves the room", || {
        alice.ui.state().user_names() == ["alice", "carol"]
    })
    .await;
    // The one wait for something not to happen: carol's connection to bob
    // closes along with his to the host.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let told = |line: &String| line.contains("provider of the file disconnected");
    assert!(
        !carol.ui.state().messages.iter().any(told),
        "carol heard of a provider she no longer streams from"
    );
}

/// A client that joins after another one started offering the video learns
/// of the offer from the room and streams the file from that client.
#[tokio::test(flavor = "multi_thread")]
async fn a_late_joiner_streams_from_the_client_that_offers_the_video() {
    let ep1 = Episode::on_disk();
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join_with_files(&relay, "bob", ep1.files());
    eventually("alice sees bob", || {
        alice.ui.state().user_names() == ["alice", "bob"]
    })
    .await;
    select(&alice, &["ep1"], "ep1");
    eventually("bob has the video selected", || {
        bob.player.state().video == Some(Video::from("ep1"))
    })
    .await;
    bob.ui.act(ui::FileShareChange {});
    eventually("alice sees bob offering it", || {
        alice.ui.state().offering() == ["bob"]
    })
    .await;

    let carol = Peer::join(&relay, "carol", &[]);
    let source = ep1.request(&bob, &carol).await;
    let received = download(&source).await;
    assert!(received == ep1.content, "the stream differs from the file");
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
    // carol plays the stream: her player reads it, over a direct link to bob
    let source = ep1.open(&bob, &carol).await;
    assert!(download(&source).await == ep1.content);
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
    eventually("bob's offer left with him", || {
        carol.ui.state().offering().is_empty()
    })
    .await;
    assert_eq!(told(), 1);
}

/// A playlist of `len` episodes with names as long as a release's: 100
/// bytes each, half a megabyte for 5,000 of them.
fn long_playlist(len: usize) -> Playlist {
    let name = |i: usize| {
        format!(
            "[Release Group] A Fairly Long Series Title S{:02}E{:03} - The Episode Title (1080p BluRay x265 10bit).mkv",
            i / 100,
            i % 100
        )
    };
    let names: Vec<String> = (0..len).map(name).collect();
    names.iter().map(String::as_str).collect()
}

/// A playlist of half a megabyte reaches every member, edits of it travel
/// both ways, a joiner is greeted with it, and the room's states and
/// heartbeats keep flowing beside it.
#[tokio::test(flavor = "multi_thread")]
async fn a_room_with_5000_episodes_keeps_working() {
    let relay = Relay::start();
    let alice = Peer::join(&relay, "alice", &[]).hosting().await;
    let bob = Peer::join(&relay, "bob", &[]);
    eventually("alice sees bob", || {
        alice.ui.state().user_names() == ["alice", "bob"]
    })
    .await;

    let playlist = long_playlist(5_000);
    let first = playlist.get(0).unwrap().clone();
    alice.ui.act(ui::PlaylistChange {
        playlist: playlist.clone(),
    });
    alice.ui.act(ui::VideoChange {
        video: first.clone(),
    });
    eventually("bob has alice's playlist", || {
        bob.ui.state().playlist == playlist
    })
    .await;
    eventually("bob loads alice's selection", || {
        bob.player.state().video == Some(first.clone())
    })
    .await;

    alice.player.seek(Duration::from_secs(600));
    eventually("bob follows alice's seek", || {
        bob.player.state().position == Duration::from_secs(600)
    })
    .await;
    // only a heartbeat brings bob back once his player went its own way
    bob.player.drift(Duration::from_secs(700));
    eventually("a heartbeat brings bob back to alice", || {
        bob.player.state().position == Duration::from_secs(600)
    })
    .await;

    let mut edited = playlist.clone();
    edited.remove(edited.len() - 1);
    bob.ui.act(ui::PlaylistChange {
        playlist: edited.clone(),
    });
    eventually("alice has bob's edit", || {
        alice.ui.state().playlist == edited
    })
    .await;
    eventually("bob shows the room's playlist", || {
        bob.ui.state().playlist == edited
    })
    .await;

    let carol = Peer::join(&relay, "carol", &[]);
    eventually("carol is greeted with the playlist", || {
        carol.ui.state().playlist == edited
    })
    .await;
    eventually("carol is where alice is", || {
        carol.player.state().position == Duration::from_secs(600)
    })
    .await;
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
        self.request(provider, viewer).await
    }

    /// `viewer` sees `provider` offering the selected episode and requests
    /// it. Returns the URL of the viewer's video server.
    async fn request(&self, provider: &Peer, viewer: &Peer) -> String {
        eventually(
            "the viewer sees the provider offering its selection",
            || {
                viewer.player.state().video == Some(Video::from("ep1"))
                    && viewer.ui.state().offering() == [provider.name()]
            },
        )
        .await;

        viewer.ui.act(ui::FileRequest {});
        eventually("the viewer's player is pointed at its video server", || {
            viewer.player.state().source.is_some()
        })
        .await;
        viewer.player.state().source.unwrap()
    }
}
