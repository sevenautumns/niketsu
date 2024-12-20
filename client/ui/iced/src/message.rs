use enum_dispatch::enum_dispatch;
use iced::Task;

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
    ToggleReady,
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

#[derive(Debug, Clone)]
pub struct ToggleReady;

impl MessageHandler for ToggleReady {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        model.model.user_ready_toggle();
        Task::none()
    }
}
