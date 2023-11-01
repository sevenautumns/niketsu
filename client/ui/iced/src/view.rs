use std::pin::Pin;

use futures::Future;
use iced::{Application, Command, Element, Renderer, Settings, Subscription, Theme};
use niketsu_core::config::Config;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{UiModel, UserInterface};
use niketsu_core::user::UserStatus;
use tokio::sync::Notify;

use super::main_window::MainView;
use super::message::Message;
use super::widget::chat::ChatWidgetState;
use super::widget::database::DatabaseWidgetState;
use super::widget::playlist::PlaylistWidgetState;
use super::widget::rooms::RoomsWidgetState;
use super::widget::settings::SettingsWidgetState;
use super::PreExistingTokioRuntime;
use crate::message::{EventOccured, MessageHandler, ModelChanged};
use crate::widget::file_search::FileSearchWidgetState;

#[derive(Debug)]
pub struct ViewModel {
    pub model: UiModel,
    pub main: MainView,
    pub settings_widget_state: SettingsWidgetState,
    pub rooms_widget_state: RoomsWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub chat_widget_statet: ChatWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub file_search_widget_state: FileSearchWidgetState,
}

impl ViewModel {
    pub fn new(flags: Flags) -> Self {
        let mut settings = SettingsWidgetState::new(flags.config.clone());
        if !flags.config.auto_connect {
            settings.activate();
        }
        Self {
            model: flags.ui_model,
            settings_widget_state: settings,
            rooms_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            chat_widget_statet: Default::default(),
            database_widget_state: Default::default(),
            file_search_widget_state: Default::default(),
            main: MainView,
        }
    }

    pub fn view(&self) -> Element<'_, Message, Renderer<Theme>> {
        self.main.view(self)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        Command::batch([message.handle(self), self.get_chat_widget_state().snap()])
    }

    pub fn user(&self) -> UserStatus {
        self.model.user.get_inner()
    }

    pub fn playing_video(&self) -> Option<Video> {
        self.model.playing_video.get_inner()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn get_rooms_widget_state(&self) -> &RoomsWidgetState {
        &self.rooms_widget_state
    }

    pub fn get_playlist_widget_state(&self) -> &PlaylistWidgetState {
        &self.playlist_widget_state
    }

    pub fn get_chat_widget_state(&self) -> &ChatWidgetState {
        &self.chat_widget_statet
    }

    pub fn get_database_widget_state(&self) -> &DatabaseWidgetState {
        &self.database_widget_state
    }

    pub fn get_file_search_widget_state(&self) -> &FileSearchWidgetState {
        &self.file_search_widget_state
    }

    pub fn get_settings_widget_state(&self) -> &SettingsWidgetState {
        &self.settings_widget_state
    }

    pub fn update_from_inner_model(&mut self) {
        self.model
            .room_list
            .on_change(|rooms| self.rooms_widget_state.replace_rooms(rooms));
        self.model
            .playlist
            .on_change(|playlist| self.playlist_widget_state.replace_playlist(playlist));
        self.model.file_database.on_change(|store| {
            self.playlist_widget_state.update_file_store(store.clone());
            self.database_widget_state.update_file_store(store)
        });
        self.model
            .file_database_status
            .on_change(|ratio| self.database_widget_state.update_progress(ratio));
        self.model
            .messages
            .on_change_arc(|msgs| self.chat_widget_statet.replace_messages(msgs))
    }
}

pub struct Flags {
    pub config: Config,
    pub ui_model: UiModel,
}

pub struct View {
    view_model: ViewModel,
}

impl View {
    pub fn create(
        config: Config,
    ) -> (
        UserInterface,
        Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
    ) {
        let ui = UserInterface::new(&config);
        let flags = Flags {
            config,
            ui_model: ui.model().clone(),
        };
        let settings = Settings::with_flags(flags);
        let view = Box::pin(async { View::run(settings).map_err(anyhow::Error::from) });
        (ui, view)
    }
}

impl Application for View {
    type Executor = PreExistingTokioRuntime;

    type Message = Message;

    type Theme = Theme;

    type Flags = Flags;

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let window = Self {
            view_model: ViewModel::new(flags),
        };
        (window, Command::none())
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        self.view_model.update(message)
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        self.view_model.view()
    }

    fn theme(&self) -> Self::Theme {
        self.view_model.theme()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let notify = self.view_model.model.notify.clone();
        let notify_subscription = iced::subscription::channel(
            std::any::TypeId::of::<Notify>(),
            1,
            |mut sender| async move {
                loop {
                    notify.notified().await;
                    let _ = sender.try_send(ModelChanged.into());
                }
            },
        );

        let ready_subscription = iced::subscription::events().map(|e| EventOccured(e).into());

        iced::subscription::Subscription::batch([notify_subscription, ready_subscription])
    }
}
