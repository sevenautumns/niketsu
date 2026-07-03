use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;

use futures::Future;
use iced::advanced::subscription::Recipe;
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::widget::pane_grid;
use iced::{Element, Event, Subscription, Task, Theme, event, window};
use niketsu_core::config::Config;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{UiModel, UserInterface};
use niketsu_core::user::UserStatus;
use tokio::sync::Notify;

use super::message::Message;
use super::widget::chat::ChatWidgetState;
use super::widget::database::DatabaseWidgetState;
use super::widget::playlist::{self, PlaylistWidgetState};
use super::widget::rooms::UsersWidgetState;
use super::widget::settings::SettingsWidgetState;
use super::{PreExistingTokioRuntime, main_window};
use crate::config::IcedConfig;
use crate::main_window::PaneKind;
use crate::message::{KeyPress, MessageHandler, ModelChanged};
use crate::widget::file_search::message::{Close as CloseFileSearch, FileSearchWidgetMessage};
use crate::widget::file_search::{self, FileSearchWidgetState};
use crate::widget::playlist::message::{CloseContext, PlaylistWidgetMessage};
use crate::widget::settings::message::{Abort, SettingsWidgetMessage};
use crate::widget::user_actions::message::{Close as CloseUserActions, UserActionsWidgetMessage};
use crate::widget::user_actions::{self, UserActionsWidgetState};
use crate::widget::{modal, settings};

#[derive(Debug)]
pub struct ViewModel {
    pub model: UiModel,
    pub settings_widget_state: SettingsWidgetState,
    pub users_widget_state: UsersWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub chat_widget_state: ChatWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub file_search_widget_state: FileSearchWidgetState,
    pub user_actions_widget_state: UserActionsWidgetState,
    pub panes: pane_grid::State<PaneKind>,
}

impl ViewModel {
    pub fn new(flags: Flags) -> Self {
        let panes = pane_grid::State::with_configuration(pane_grid::Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: flags.iced_config.pane_ratio,
            a: Box::new(pane_grid::Configuration::Pane(PaneKind::Chat)),
            b: Box::new(pane_grid::Configuration::Pane(PaneKind::Controls)),
        });
        let mut settings = SettingsWidgetState::new(flags.config.clone(), flags.iced_config);
        if !flags.config.auto_connect {
            settings.activate();
        }
        Self {
            model: flags.ui_model,
            settings_widget_state: settings,
            users_widget_state: Default::default(),
            playlist_widget_state: Default::default(),
            chat_widget_state: Default::default(),
            database_widget_state: Default::default(),
            file_search_widget_state: Default::default(),
            user_actions_widget_state: Default::default(),
            panes,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let base = main_window::view(self);
        let overlay: Option<(Element<'_, Message>, Message)> =
            if self.settings_widget_state.is_active() {
                Some((
                    settings::view(&self.settings_widget_state),
                    SettingsWidgetMessage::from(Abort).into(),
                ))
            } else if self.file_search_widget_state.is_active() {
                Some((
                    file_search::view(&self.file_search_widget_state),
                    FileSearchWidgetMessage::from(CloseFileSearch).into(),
                ))
            } else if self.user_actions_widget_state.is_active() {
                Some((
                    user_actions::view(&self.user_actions_widget_state),
                    UserActionsWidgetMessage::from(CloseUserActions).into(),
                ))
            } else if self.playlist_widget_state.context_active() {
                Some((
                    playlist::context_view(&self.playlist_widget_state),
                    PlaylistWidgetMessage::from(CloseContext).into(),
                ))
            } else {
                None
            };
        modal(base, overlay)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        message.handle(self)
    }

    pub fn user(&self) -> UserStatus {
        self.model.user.get_inner()
    }

    pub fn playing_video(&self) -> Option<Video> {
        self.model.playing_video.get_inner()
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
            .on_change_arc(|msgs| self.chat_widget_state.replace_messages(msgs))
    }

    pub fn is_sharing(&self) -> bool {
        self.model.video_share.get_inner()
    }

    pub fn is_host(&self) -> bool {
        self.model.is_host.get_inner()
    }
}

#[derive(Clone)]
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
            iced::application(
                move || View {
                    view_model: ViewModel::new(flags.clone()),
                },
                Self::update,
                Self::view,
            )
            .title("Niketsu")
            .theme(Self::theme)
            .subscription(Self::subscription)
            .executor::<PreExistingTokioRuntime>()
            .run()
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
        Subscription::batch([
            iced::advanced::subscription::from_recipe(model_subscription),
            event::listen_with(key_press),
        ])
    }
}

fn key_press(event: Event, status: event::Status, _window: window::Id) -> Option<Message> {
    let Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: Key::Named(key),
        ..
    }) = event
    else {
        return None;
    };
    matches!(
        key,
        Named::Space | Named::Escape | Named::Enter | Named::ArrowUp | Named::ArrowDown
    )
    .then(|| {
        KeyPress {
            key,
            captured: matches!(status, event::Status::Captured),
        }
        .into()
    })
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
