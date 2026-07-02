use enum_dispatch::enum_dispatch;
use iced::Task;
use iced::keyboard::key::Named;

use super::main_window::message::MainMessage;
use super::widget::chat::message::ChatWidgetMessage;
use super::widget::database::message::DatabaseWidgetMessage;
use super::widget::playlist::message::PlaylistWidgetMessage;
use crate::view::ViewModel;
use crate::widget::file_search::message::FileSearchWidgetMessage;
use crate::widget::settings::message::SettingsWidgetMessage;

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
    //
    SettingsWidget(SettingsWidgetMessage),
    PlaylistWidget(PlaylistWidgetMessage),
    ChatWidget(ChatWidgetMessage),
    DatabaseWidget(DatabaseWidgetMessage),
    FileSearchWidget(FileSearchWidgetMessage),
}

#[derive(Debug, Clone, Copy)]
pub struct ModelChanged;

impl MessageHandler for ModelChanged {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        model.update_from_inner_model();
        if !model.model.running.get_inner() {
            return iced::exit();
        }
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
        if !self.captured && self.key == Named::Space {
            model.model.user_ready_toggle();
        }
        Task::none()
    }
}
