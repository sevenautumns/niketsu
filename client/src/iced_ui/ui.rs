use std::path::PathBuf;

use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::warn;
use tokio::sync::Notify;

use super::main::MainView;
use super::message::Message;
use super::settings::SettingsView;
use super::widget::database::DatabaseWidgetState;
use super::widget::messages::MessagesWidgetState;
use super::widget::playlist::PlaylistWidgetState;
use super::widget::rooms::RoomsWidgetState;
use super::{PreExistingTokioRuntime, UiModel};
use crate::config::Config;
use crate::core::playlist::PlaylistVideo;
use crate::core::ui::{RoomChange, ServerChange};
use crate::core::user::UserStatus;

#[derive(Debug)]
pub struct ViewModel {
    model: UiModel,
    config: Config,
    main: MainView,
    settings: Option<SettingsView>,
    rooms_widget_state: RoomsWidgetState,
    playlist_widget_state: PlaylistWidgetState,
    messages_widget_state: MessagesWidgetState,
    database_widget_state: DatabaseWidgetState,
}

impl ViewModel {
    pub fn new(model: UiModel, config: Config) -> Self {
        Self {
            model,
            settings: Some(SettingsView::from(config.clone())),
            config,
            main: Default::default(),
            rooms_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            messages_widget_state: Default::default(),
            database_widget_state: Default::default(),
        }
    }

    pub fn view(&self) -> Element<'_, Message, Renderer<Theme>> {
        if let Some(settings) = &self.settings {
            return settings.view(self);
        }
        self.main.view(self)
    }

    fn update(&mut self, message: Message) {
        // TODO use boxes?
        match message {
            Message::Settings(message) => match &mut self.settings {
                Some(settings) => settings.update(message, &self.model),
                None => warn!("unhandled settings message: {message:?}"),
            },
            Message::Main(m) => self.main.update(m, &self.model),
            Message::CloseSettings => self.close_settings(),
            Message::RoomsWidget(m) => m.handle(&mut self.rooms_widget_state, &self.model),
            Message::PlaylistWidget(m) => m.handle(&mut self.playlist_widget_state, &self.model),
            Message::MessagesWidget(m) => m.handle(&mut self.messages_widget_state),
            Message::DatabaseWidget(m) => m.handle(&self.model),
            Message::ModelChanged => self.update_from_inner_model(),
        }
    }

    fn close_settings(&mut self) {
        let Some(settings) = self.settings.take() else {
            return;
        };
        let config: Config = settings.into();
        let media_dirs: Vec<_> = config.media_dirs.iter().map(PathBuf::from).collect();
        let username = config.username.clone();
        self.model.change_db_paths(media_dirs);
        self.model.change_username(username);
        self.model.change_server(ServerChange {
            addr: config.url.clone(),
            secure: config.secure,
            password: Some(config.password.clone()),
            room: RoomChange {
                room: config.room.clone(),
            },
        });
        self.config = config;
        crate::log!(self.config.save());
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn user(&self) -> UserStatus {
        self.model.user.get_inner()
    }

    pub fn playing_video(&self) -> Option<PlaylistVideo> {
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

pub struct View {
    view_model: ViewModel,
}

pub trait SubWindowTrait {
    type SubMessage;

    fn view(&self, model: &ViewModel) -> Element<Message>;
    fn update(&mut self, message: Self::SubMessage, model: &UiModel);
}

impl Application for View {
    type Executor = PreExistingTokioRuntime;

    type Message = Message;

    type Theme = Theme;

    type Flags = (Config, UiModel);

    fn new((config, model): Self::Flags) -> (Self, Command<Self::Message>) {
        let window = Self {
            view_model: ViewModel::new(model, config),
        };
        (window, Command::none())
    }

    fn title(&self) -> String {
        "Niketsu".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        self.view_model.update(message);
        Command::none()
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
                (Self::Message::ModelChanged, notify)
            },
        )
    }
}
