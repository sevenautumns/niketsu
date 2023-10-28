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
use super::settings_window::SettingsViewState;
use super::widget::database::DatabaseWidgetState;
use super::widget::messages::MessagesWidgetState;
use super::widget::playlist::PlaylistWidgetState;
use super::widget::rooms::RoomsWidgetState;
use super::PreExistingTokioRuntime;
use crate::message::{MessageHandler, ModelChanged};
use crate::widget::file_search::FileSearchWidgetState;

#[derive(Debug)]
pub struct ViewModel {
    pub model: UiModel,
    pub main: MainView,
    pub settings: SettingsViewState,
    pub rooms_widget_state: RoomsWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub messages_widget_state: MessagesWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub file_search_widget_state: FileSearchWidgetState,
}

impl ViewModel {
    pub fn new(flags: Flags) -> Self {
        let mut settings = SettingsViewState::new(flags.config.clone());
        if !flags.config.auto_login {
            settings.activate();
        }
        Self {
            model: flags.ui_model,
            settings,
            rooms_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            messages_widget_state: Default::default(),
            database_widget_state: Default::default(),
            file_search_widget_state: Default::default(),
            main: MainView,
        }
    }

    pub fn view(&self) -> Element<'_, Message, Renderer<Theme>> {
        self.main.view(self)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        message.handle(self)
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

    pub fn get_messages_widget_state(&self) -> &MessagesWidgetState {
        &self.messages_widget_state
    }

    pub fn get_database_widget_state(&self) -> &DatabaseWidgetState {
        &self.database_widget_state
    }

    pub fn get_file_search_widget_state(&self) -> &FileSearchWidgetState {
        &self.file_search_widget_state
    }

    pub fn get_settings_view_state(&self) -> &SettingsViewState {
        &self.settings
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
            .on_change_arc(|msgs| self.messages_widget_state.replace_messages(msgs))
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
        let ui = UserInterface::default();
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
        iced::subscription::unfold(
            std::any::TypeId::of::<Notify>(),
            self.view_model.model.notify.clone(),
            |notify| async {
                notify.notified().await;
                (ModelChanged.into(), notify)
            },
        )
    }
}
