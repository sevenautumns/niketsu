use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;

use futures::Future;
use iced::advanced::subscription::Recipe;
use iced::{Element, Subscription, Task, Theme};
use niketsu_core::config::Config;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{UiModel, UserInterface};
use niketsu_core::user::UserStatus;
use tokio::sync::Notify;

use super::PreExistingTokioRuntime;
use super::main_window::MainView;
use super::message::Message;
use super::widget::chat::ChatWidgetState;
use super::widget::database::DatabaseWidgetState;
use super::widget::playlist::PlaylistWidgetState;
use super::widget::rooms::UsersWidgetState;
use super::widget::settings::SettingsWidgetState;
use crate::config::IcedConfig;
use crate::message::{MessageHandler, ModelChanged};
use crate::widget::file_search::FileSearchWidgetState;

#[derive(Debug)]
pub struct ViewModel {
    pub model: UiModel,
    pub settings_widget_state: SettingsWidgetState,
    pub users_widget_state: UsersWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub chat_widget_statet: ChatWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub file_search_widget_state: FileSearchWidgetState,
}

impl ViewModel {
    pub fn new(flags: Flags) -> Self {
        let mut settings = SettingsWidgetState::new(flags.config.clone(), flags.iced_config);
        if !flags.config.auto_connect {
            settings.activate();
        }
        Self {
            model: flags.ui_model,
            settings_widget_state: settings,
            users_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            chat_widget_statet: Default::default(),
            database_widget_state: Default::default(),
            file_search_widget_state: Default::default(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        MainView::new(self).into()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        Task::batch([message.handle(self), self.get_chat_widget_state().snap()])
    }

    pub fn user(&self) -> UserStatus {
        self.model.user.get_inner()
    }

    pub fn playing_video(&self) -> Option<Video> {
        self.model.playing_video.get_inner()
    }

    pub fn get_rooms_widget_state(&self) -> &UsersWidgetState {
        &self.users_widget_state
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
            .user_list
            .on_change(|rooms| self.users_widget_state.replace_users(rooms));
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
    pub iced_config: IcedConfig,
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
            iced_config: IcedConfig::load_or_default(),
            ui_model: ui.model().clone(),
        };
        let view = Box::pin(async {
            iced::application("Niketsu", Self::update, Self::view)
                .theme(Self::theme)
                .subscription(Self::subscription)
                .executor::<PreExistingTokioRuntime>()
                .run_with(|| {
                    (
                        View {
                            view_model: ViewModel::new(flags),
                        },
                        Task::none(),
                    )
                })
                .map_err(anyhow::Error::from)
        });
        (ui, view)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.view_model.update(message)
    }

    fn view(&self) -> Element<'_, Message> {
        self.view_model.view()
    }

    fn theme(&self) -> Theme {
        self.view_model
            .settings_widget_state
            .iced_config()
            .theme
            .clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        let notify = self.view_model.model.notify.clone();
        let model_subscription = ModelSubscription { notify };
        iced::advanced::subscription::from_recipe(model_subscription)
    }
}

pub struct ModelSubscription {
    notify: Arc<Notify>,
}

impl Recipe for ModelSubscription {
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        std::any::TypeId::of::<Self>().hash(state)
    }

    fn stream(
        self: Box<Self>,
        _: iced::advanced::subscription::EventStream,
    ) -> futures::stream::BoxStream<'static, Self::Output> {
        Box::pin(futures::stream::unfold(self, |s| async {
            s.notify.notified().await;
            Some((ModelChanged.into(), s))
        }))
    }
}
