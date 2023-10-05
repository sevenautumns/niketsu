use std::sync::Arc;

use async_trait::async_trait;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, PlaylistVideo};
use niketsu_core::rooms::RoomList;
use niketsu_core::ui::{
    PlayerMessage, UiModel, UserChange, UserInterfaceEvent, UserInterfaceTrait,
};
use niketsu_core::util::{Observed, RingBuffer};
use tokio::sync::mpsc::UnboundedReceiver as MpscReceiver;
use tokio::sync::Notify;

mod handler;
mod view;
mod widget;

#[derive(Debug)]
pub struct RatatuiUI {
    config: Config,
    model: UiModel,
    ui_events: MpscReceiver<UserInterfaceEvent>,
}

impl RatatuiUI {
    pub fn new(config: Config) -> (Self, Box<dyn FnOnce() -> anyhow::Result<()>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let model = UiModel {
            file_database: Observed::<_>::default_with_notify(&notify),
            file_database_status: Observed::<_>::default_with_notify(&notify),
            playlist: Observed::<_>::default_with_notify(&notify),
            playing_video: Observed::<_>::default_with_notify(&notify),
            room_list: Observed::<_>::default_with_notify(&notify),
            user: Observed::<_>::default_with_notify(&notify),
            messages: Observed::new(RingBuffer::new(1000), &notify),
            events: tx,
            notify,
        };
        let mut view = view::RatatuiView::new(model.clone());
        let handle = Box::new(move || futures::executor::block_on(view.run()));
        (
            RatatuiUI {
                config,
                model,
                ui_events: rx,
            },
            handle,
        )
    }
}

#[async_trait]
impl UserInterfaceTrait for RatatuiUI {
    fn file_database_status(&mut self, update_status: f32) {
        self.model.file_database_status.set(update_status);
    }

    fn file_database(&mut self, db: FileStore) {
        self.model.file_database.set(db);
    }

    fn playlist(&mut self, playlist: Playlist) {
        self.model.playlist.set(playlist);
    }

    fn room_list(&mut self, room_list: RoomList) {
        self.model.room_list.set(room_list);
    }

    fn video_change(&mut self, video: Option<PlaylistVideo>) {
        self.model.playing_video.set(video);
    }

    fn user_update(&mut self, user: UserChange) {
        self.model.user.set(user.into());
    }

    fn player_message(&mut self, msg: PlayerMessage) {
        self.model.messages.rcu(|messages| {
            let mut messages = RingBuffer::clone(messages);
            messages.push(msg.clone());
            messages
        });
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        self.ui_events.recv().await.expect("ui event stream ended")
    }
}
