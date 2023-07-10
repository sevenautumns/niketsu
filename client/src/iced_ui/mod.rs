use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use elsa::sync::FrozenVec;
use tokio::sync::mpsc::UnboundedReceiver as MpscReceiver;
use tokio::sync::Notify;

use self::ui::IcedUiWindow;
use crate::core::ui::{PlayerMessage, UserChange, UserInterface, UserInterfaceEvent};
use crate::core::user::RoomList;

mod running;
mod startup;
mod ui;

#[derive(Clone)]
struct IcedUIData {
    file_database: Arc<ArcSwap<Vec<PathBuf>>>,
    file_database_status: Arc<ArcSwap<f32>>,
    playlist: Arc<ArcSwap<Vec<String>>>,
    room_list: Arc<ArcSwap<RoomList>>,
    user: Arc<ArcSwap<User>>,
    messages: Arc<FrozenVec<Arc<PlayerMessage>>>,
}

pub struct IcedUI {
    data: IcedUIData,
    ui: IcedUiWindow,
    iced_notify: Arc<Notify>,
    ui_events: MpscReceiver<UserInterfaceEvent>,
}

#[async_trait]
impl UserInterface for IcedUI {
    fn file_database_status(&mut self, update_status: f32) {
        self.data
            .file_database_status
            .store(Arc::new(update_status));
        self.iced_notify.notify_waiters();
    }

    fn file_database(&mut self, db: Vec<PathBuf>) {
        self.data.file_database.store(Arc::new(db));
        self.iced_notify.notify_waiters();
    }

    fn playlist(&mut self, playlist: Vec<String>) {
        self.data.playlist.store(Arc::new(playlist));
        self.iced_notify.notify_waiters();
    }

    fn room_list(&mut self, room_list: RoomList) {
        self.data.room_list.store(Arc::new(room_list));
        self.iced_notify.notify_waiters();
    }

    fn user_update(&mut self, user: UserChange) {
        self.data.user.store(Arc::new(user.into()));
        self.iced_notify.notify_waiters();
    }

    fn player_message(&mut self, msg: PlayerMessage) {
        self.data.messages.as_ref().push(Arc::new(msg));
        self.iced_notify.notify_waiters();
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        self.ui_events.recv().await.expect("ui event stream ended")
    }
}

#[derive(Debug, Clone)]
struct User {
    name: String,
    ready: bool,
}

impl From<UserChange> for User {
    fn from(value: UserChange) -> Self {
        Self {
            name: value.name,
            ready: value.ready,
        }
    }
}
