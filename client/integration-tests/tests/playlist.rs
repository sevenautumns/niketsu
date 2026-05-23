use niketsu_core::communicator::{ConnectedMsg, OutgoingMessage, PlaylistMsg};
use niketsu_core::config::Config;
use niketsu_core::playlist::Playlist;
use niketsu_core::ui::PlaylistChange;
use niketsu_integration_tests::TestClient;

/// When the UI sends a PlaylistChange, core must broadcast it over the communicator.
#[tokio::test]
async fn playlist_change_is_broadcast() {
    let mut client = TestClient::new(Config::default());

    // Simulate a peer connecting (core sends UserStatus in response; drain it)
    client.session.comm.send_incoming(ConnectedMsg { is_host: false }.into());

    let mut playlist = Playlist::default();
    playlist.push("test.mkv".into());
    client.session.ui.send_event(PlaylistChange { playlist: playlist.clone() }.into());

    // Drain messages until we see a Playlist broadcast (UserStatus arrives first)
    let msg = client
        .wait_for_outgoing(|m| matches!(m, OutgoingMessage::Playlist(_)))
        .await;

    assert!(matches!(msg, OutgoingMessage::Playlist(_)));
}

/// When an incoming PlaylistMsg arrives from a peer, core must push it to the UI.
#[tokio::test]
async fn incoming_playlist_updates_ui() {
    let mut client = TestClient::new(Config::default());

    let mut playlist = Playlist::default();
    playlist.push("movie.mkv".into());

    client.session.comm.send_incoming(
        PlaylistMsg { actor: "peer".into(), playlist: playlist.clone() }.into(),
    );

    let received = client.wait_for_playlist().await;
    assert_eq!(received, playlist);
}
