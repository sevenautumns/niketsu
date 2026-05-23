use std::time::Duration;

use niketsu_api::{CoreSession, UiNotification};
use niketsu_core::communicator::OutgoingMessage;
use niketsu_core::config::Config;
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use tokio::time::timeout;

pub struct TestClient {
    pub session: CoreSession,
}

impl TestClient {
    pub fn new(config: Config) -> Self {
        Self { session: CoreSession::start(config) }
    }

    /// Drain UI notifications until one matches `pred`, or panic after 1 second.
    pub async fn wait_for(&mut self, pred: impl Fn(&UiNotification) -> bool) -> UiNotification {
        let fut = async {
            loop {
                let notif = self.session.ui.recv().await;
                if pred(&notif) {
                    return notif;
                }
            }
        };
        timeout(Duration::from_secs(1), fut)
            .await
            .expect("wait_for: no matching notification within 1s")
    }

    /// Drain outgoing communicator messages until one matches `pred`, or panic after 1 second.
    pub async fn wait_for_outgoing(
        &mut self,
        pred: impl Fn(&OutgoingMessage) -> bool,
    ) -> OutgoingMessage {
        let fut = async {
            loop {
                let msg = self.session.comm.recv_outgoing().await;
                if pred(&msg) {
                    return msg;
                }
            }
        };
        timeout(Duration::from_secs(1), fut)
            .await
            .expect("wait_for_outgoing: no matching message within 1s")
    }

    pub async fn wait_for_playlist(&mut self) -> Playlist {
        match self.wait_for(|n| matches!(n, UiNotification::Playlist(_))).await {
            UiNotification::Playlist(p) => p,
            _ => unreachable!(),
        }
    }

    pub async fn wait_for_user_list(&mut self) -> UserList {
        match self.wait_for(|n| matches!(n, UiNotification::UserList(_))).await {
            UiNotification::UserList(u) => u,
            _ => unreachable!(),
        }
    }

    pub async fn wait_for_video_change(&mut self) -> Option<Video> {
        match self.wait_for(|n| matches!(n, UiNotification::VideoChange(_))).await {
            UiNotification::VideoChange(v) => v,
            _ => unreachable!(),
        }
    }
}
