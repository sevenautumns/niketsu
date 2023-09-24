use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use iced::{Application, Executor, Settings};
use niketsu_core::config::Config as CoreConfig;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, PlaylistVideo};
use niketsu_core::rooms::RoomList;
use niketsu_core::ui::{
    PlayerMessage, UiModel, UserChange, UserInterfaceEvent, UserInterfaceTrait,
};
use niketsu_core::util::{Observed, RingBuffer};
use once_cell::sync::Lazy;
use tokio::sync::mpsc::UnboundedReceiver as MpscReceiver;
use tokio::sync::Notify;
use view::Flags;

use self::view::View;
use crate::config::Config;

pub mod config;
mod main_window;
mod message;
mod settings_window;
mod styling;
mod view;
mod widget;

pub static TEXT_SIZE: Lazy<ArcSwap<f32>> = Lazy::new(|| ArcSwap::new(Arc::new(14.0)));

#[derive(Debug)]
pub struct IcedUI {
    model: UiModel,
    ui_events: MpscReceiver<UserInterfaceEvent>,
}

impl IcedUI {
    pub fn new(
        config: Config,
        core_config: CoreConfig,
    ) -> (Self, Box<dyn FnOnce() -> anyhow::Result<()>>) {
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
        let flags = Flags {
            config,
            core_config,
            ui_model: model.clone(),
        };
        let settings = Settings::with_flags(flags);
        let view = Box::new(move || View::run(settings).map_err(anyhow::Error::from));
        (
            Self {
                model,
                ui_events: rx,
            },
            view,
        )
    }
}

#[async_trait]
impl UserInterfaceTrait for IcedUI {
    fn file_database_status(&mut self, update_status: f32) {
        self.model.file_database_status.set(update_status);
    }

    fn file_database(&mut self, db: FileStore) {
        self.model.file_database.set(db);
    }

    fn playlist(&mut self, playlist: Playlist) {
        self.model.playlist.set(playlist);
    }

    fn video_change(&mut self, video: Option<PlaylistVideo>) {
        self.model.playing_video.set(video);
    }

    fn room_list(&mut self, room_list: RoomList) {
        self.model.room_list.set(room_list);
    }

    fn user_update(&mut self, user: UserChange) {
        self.model.user.set(user.into());
    }

    fn player_message(&mut self, msg: PlayerMessage) {
        self.model.messages.rcu(|msgs| {
            let mut msgs = RingBuffer::clone(msgs);
            msgs.push(msg.clone());
            msgs
        });
    }

    async fn event(&mut self) -> UserInterfaceEvent {
        self.ui_events.recv().await.expect("ui event stream ended")
    }
}

#[derive(Debug)]
pub struct PreExistingTokioRuntime;

impl Executor for PreExistingTokioRuntime {
    fn new() -> Result<Self, futures::io::Error>
    where
        Self: Sized,
    {
        Ok(Self)
    }

    #[allow(clippy::let_underscore_future)]
    fn spawn(&self, future: impl futures::Future<Output = ()> + iced_futures::MaybeSend + 'static) {
        let _ = tokio::task::spawn(future);
    }
}
