use std::path::PathBuf;
use std::pin::Pin;

use futures::Future;
use iced::{Application, Command, Element, Renderer, Settings, Subscription, Theme};
use niketsu_core::config::Config as CoreConfig;
use niketsu_core::log;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{RoomChange, ServerChange, UiModel, UserInterface};
use niketsu_core::user::UserStatus;
use tokio::sync::Notify;

use super::main_window::MainView;
use super::message::Message;
use super::settings_window::SettingsView;
use super::widget::database::DatabaseWidgetState;
use super::widget::messages::MessagesWidgetState;
use super::widget::playlist::PlaylistWidgetState;
use super::widget::rooms::RoomsWidgetState;
use super::PreExistingTokioRuntime;
use crate::config::Config;
use crate::message::{MessageHandler, ModelChanged};
use crate::widget::file_search::FileSearchWidgetState;

#[derive(Debug)]
pub struct ViewModel {
    pub model: UiModel,
    pub config: Config,
    pub core_config: CoreConfig,
    pub main: MainView,
    pub settings: Option<SettingsView>,
    pub rooms_widget_state: RoomsWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub messages_widget_state: MessagesWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub file_search_widget_state: FileSearchWidgetState,
}

impl ViewModel {
    pub fn new(flags: Flags) -> Self {
        let settings = SettingsView::new(flags.config.clone(), flags.core_config.clone());
        let mut view = Self {
            model: flags.ui_model,
            config: flags.config,
            core_config: flags.core_config.clone(),
            settings: Some(settings),
            main: Default::default(),
            rooms_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            messages_widget_state: Default::default(),
            database_widget_state: Default::default(),
            file_search_widget_state: Default::default(),
        };
        if flags.core_config.auto_login {
            view.close_settings()
        }
        view
    }

    pub fn view(&self) -> Element<'_, Message, Renderer<Theme>> {
        if let Some(settings) = &self.settings {
            return settings.view(self);
        }
        self.main.view(self)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        message.handle(self)
    }

    pub fn close_settings(&mut self) {
        let Some(settings) = self.settings.take() else {
            return;
        };
        let (config, core): (Config, CoreConfig) = settings.into_config();
        let media_dirs: Vec<_> = core.media_dirs.iter().map(PathBuf::from).collect();
        let username = core.username.clone();
        self.model.change_db_paths(media_dirs);
        self.model.change_username(username);
        self.model.change_server(ServerChange {
            addr: core.url.clone(),
            secure: core.secure,
            password: Some(core.password.clone()),
            room: RoomChange {
                room: core.room.clone(),
            },
        });
        self.config = config;
        self.core_config = core;
        log!(self.config.save());
        log!(self.core_config.save());
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn user(&self) -> UserStatus {
        self.model.user.get_inner()
    }

    pub fn playing_video(&self) -> Option<Video> {
        self.model.playing_video.get_inner()
    }

    pub fn theme(&self) -> Theme {
        self.config.theme()
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
    pub core_config: CoreConfig,
    pub ui_model: UiModel,
}

pub struct View {
    view_model: ViewModel,
}

impl View {
    pub fn create(
        config: Config,
        core_config: CoreConfig,
    ) -> (
        UserInterface,
        Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
    ) {
        let ui = UserInterface::default();
        let flags = Flags {
            config,
            core_config,
            ui_model: ui.model().clone(),
        };
        let settings = Settings::with_flags(flags);
        let view = Box::pin(async { View::run(settings).map_err(anyhow::Error::from) });
        (ui, view)
    }
}

pub trait SubWindowTrait {
    type SubMessage;

    fn view<'a>(&'a self, model: &'a ViewModel) -> Element<Message>;
    fn update(&mut self, message: Self::SubMessage, model: &UiModel);
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
