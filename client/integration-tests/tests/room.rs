use niketsu_core::communicator::{ConnectedMsg, OutgoingMessage};
use niketsu_core::config::Config;
use niketsu_core::ui::RoomChange;
use niketsu_integration_tests::TestClient;

/// Sending RoomChange from the UI must cause the communicator to register as connected.
#[tokio::test]
async fn room_change_triggers_connect() {
    let client = TestClient::new(Config::default());
    assert!(!client.session.comm.is_connected());

    client.session.ui.send_event(
        RoomChange { room: "test-room".into(), password: String::new() }.into(),
    );

    // Core processes events asynchronously — yield briefly so the select loop runs
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(client.session.comm.is_connected());
}

/// After connection is established, core must broadcast the local user status to peers.
#[tokio::test]
async fn connected_triggers_user_status_broadcast() {
    let mut client = TestClient::new(Config::default());

    client.session.comm.send_incoming(ConnectedMsg { is_host: false }.into());

    let msg = client
        .wait_for_outgoing(|m| matches!(m, OutgoingMessage::UserStatus(_)))
        .await;

    assert!(matches!(msg, OutgoingMessage::UserStatus(_)));
}

/// After connection as host, core must notify the UI of the host role.
#[tokio::test]
async fn connected_as_host_notifies_ui() {
    use niketsu_api::UiNotification;

    let mut client = TestClient::new(Config::default());

    client.session.comm.send_incoming(ConnectedMsg { is_host: true }.into());

    let notif = client.wait_for(|n| matches!(n, UiNotification::IsHost(_))).await;

    assert!(matches!(notif, UiNotification::IsHost(true)));
}
