use arcstr::ArcStr;
use async_trait::async_trait;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, Video};
use niketsu_core::room::UserList;
use niketsu_core::ui::{PlayerMessage, UserChange, UserInterfaceEvent, UserInterfaceTrait};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone)]
pub enum UiNotification {
    FileDatabaseStatus(f32),
    FileDatabase(FileStore),
    Playlist(Playlist),
    VideoChange(Option<Video>),
    UserList(UserList),
    UserUpdate(UserChange),
    PlayerMessage(PlayerMessage),
    UsernameChange(ArcStr),
    Abort,
    VideoShare(bool),
    IsHost(bool),
}

pub struct UiHandle {
    pub(crate) notif_rx: UnboundedReceiver<UiNotification>,
    pub(crate) event_tx: UnboundedSender<UserInterfaceEvent>,
}

impl UiHandle {
    pub async fn recv(&mut self) -> UiNotification {
        self.notif_rx.recv().await.expect("ApiUi dropped")
    }

    pub fn recv_blocking(&mut self) -> UiNotification {
        self.notif_rx.blocking_recv().expect("ApiUi dropped")
    }

    pub fn try_recv(&mut self) -> Option<UiNotification> {
        self.notif_rx.try_recv().ok()
    }

    pub fn send_event(&self, event: UserInterfaceEvent) {
        let _ = self.event_tx.send(event);
    }
}

#[derive(Debug)]
pub struct ApiUi {
    notif_tx: UnboundedSender<UiNotification>,
    event_rx: UnboundedReceiver<UserInterfaceEvent>,
}

pub fn api_ui() -> (ApiUi, UiHandle) {
    let (notif_tx, notif_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    (
        ApiUi { notif_tx, event_rx },
        UiHandle { notif_rx, event_tx },
    )
}

#[async_trait]
impl UserInterfaceTrait for ApiUi {
    fn file_database_status(&mut self, update_status: f32) {
        let _ = self.notif_tx.send(UiNotification::FileDatabaseStatus(update_status));
    }

    fn file_database(&mut self, db: FileStore) {
        let _ = self.notif_tx.send(UiNotification::FileDatabase(db));
    }

    fn playlist(&mut self, playlist: Playlist) {
        let _ = self.notif_tx.send(UiNotification::Playlist(playlist));
    }

    fn video_change(&mut self, video: Option<Video>) {
        let _ = self.notif_tx.send(UiNotification::VideoChange(video));
    }

    fn user_list(&mut self, user_list: UserList) {
        let _ = self.notif_tx.send(UiNotification::UserList(user_list));
    }

    fn user_update(&mut self, user: UserChange) {
        let _ = self.notif_tx.send(UiNotification::UserUpdate(user));
    }

    fn player_message(&mut self, msg: PlayerMessage) {
        let _ = self.notif_tx.send(UiNotification::PlayerMessage(msg));
    }

    fn username_change(&mut self, username: ArcStr) {
        let _ = self.notif_tx.send(UiNotification::UsernameChange(username));
    }

    fn abort(&mut self) {
        let _ = self.notif_tx.send(UiNotification::Abort);
    }

    fn video_share(&mut self, video_share: bool) {
        let _ = self.notif_tx.send(UiNotification::VideoShare(video_share));
    }

    fn is_host(&mut self, is_host: bool) {
        let _ = self.notif_tx.send(UiNotification::IsHost(is_host));
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        self.event_rx.recv().await.expect("UiHandle dropped")
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::ui::PlaylistChange;

    use super::*;

    #[tokio::test]
    async fn push_methods_send_notifications() {
        let (mut api_ui, mut handle) = api_ui();

        api_ui.is_host(true);
        api_ui.video_share(false);

        assert!(matches!(handle.try_recv(), Some(UiNotification::IsHost(true))));
        assert!(matches!(handle.try_recv(), Some(UiNotification::VideoShare(false))));
    }

    #[tokio::test]
    async fn send_event_is_received_by_event() {
        let (mut api_ui, handle) = api_ui();

        handle.send_event(PlaylistChange { playlist: Playlist::default() }.into());

        let event = api_ui.event().await;
        assert!(matches!(event, UserInterfaceEvent::PlaylistChange(_)));
    }
}
