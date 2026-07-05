use enum_dispatch::enum_dispatch;
use iced::Task;
use iced::keyboard::key::Named;
use iced::widget::pane_grid;
use niketsu_core::log_err;

use super::main_window::message::MainMessage;
use super::widget::chat::message::ChatWidgetMessage;
use super::widget::database::message::DatabaseWidgetMessage;
use super::widget::playlist::message::{CloseContext, PlaylistWidgetMessage};
use crate::view::ViewModel;
use crate::widget::file_search::message::{FileSearchWidgetMessage, KeyInput};
use crate::widget::settings::message::{Abort, SettingsWidgetMessage};
use crate::widget::user_actions::message::{Close as CloseUserActions, UserActionsWidgetMessage};

#[enum_dispatch]
pub trait MessageHandler {
    fn handle(self, model: &mut ViewModel) -> Task<Message>;
}

#[enum_dispatch(MessageHandler)]
#[derive(Debug, Clone)]
pub enum Message {
    Main(MainMessage),
    ModelChanged,
    KeyPress,
    PaneResized,
    //
    SettingsWidget(SettingsWidgetMessage),
    PlaylistWidget(PlaylistWidgetMessage),
    ChatWidget(ChatWidgetMessage),
    DatabaseWidget(DatabaseWidgetMessage),
    FileSearchWidget(FileSearchWidgetMessage),
    UserActionsWidget(UserActionsWidgetMessage),
}

#[derive(Debug, Clone, Copy)]
pub struct ModelChanged;

impl MessageHandler for ModelChanged {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        model.update_from_inner_model();
        if !model.model.running.get_inner() {
            // Persist layout changes (e.g. the pane split) made this session.
            log_err!(model.settings_widget_state.iced_config().save());
            return iced::exit();
        }
        // Keep the chat pinned to the bottom when new messages arrive.
        model.chat_widget_state.snap()
    }
}

/// The split between the chat pane and the side pane was dragged.
#[derive(Debug, Clone)]
pub struct PaneResized(pub pane_grid::ResizeEvent);

impl MessageHandler for PaneResized {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        model.panes.resize(self.0.split, self.0.ratio);
        model.settings_widget_state.set_pane_ratio(self.0.ratio);
        Task::none()
    }
}

/// A named key press observed by the runtime, along with whether some
/// widget already captured it (e.g. a focused text input).
#[derive(Debug, Clone, Copy)]
pub struct KeyPress {
    pub key: Named,
    pub captured: bool,
}

impl MessageHandler for KeyPress {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        if model.file_search_widget_state.is_active() {
            return FileSearchWidgetMessage::from(KeyInput {
                key: self.key,
                captured: self.captured,
            })
            .handle(model);
        }
        if model.settings_widget_state.is_active() {
            if self.key == Named::Escape {
                return SettingsWidgetMessage::from(Abort).handle(model);
            }
            return Task::none();
        }
        if model.user_actions_widget_state.is_active() {
            if self.key == Named::Escape {
                return UserActionsWidgetMessage::from(CloseUserActions).handle(model);
            }
            return Task::none();
        }
        if model.playlist_widget_state.context_active() {
            if self.key == Named::Escape {
                return PlaylistWidgetMessage::from(CloseContext).handle(model);
            }
            return Task::none();
        }
        if !self.captured && self.key == Named::Space {
            model.model.user_ready_toggle();
        }
        Task::none()
    }
}
