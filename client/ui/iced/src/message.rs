use enum_dispatch::enum_dispatch;
use iced::Command;

use super::main_window::message::MainMessage;
use super::widget::database::message::DatabaseWidgetMessage;
use super::widget::messages::message::MessagesWidgetMessage;
use super::widget::playlist::message::PlaylistWidgetMessage;
use super::widget::rooms::message::RoomsWidgetMessage;
use crate::settings_window::message::SettingsMessage;
use crate::view::ViewModel;
use crate::widget::file_search::message::FileSearchWidgetMessage;

#[enum_dispatch]
pub trait MessageHandler {
    fn handle(self, model: &mut ViewModel) -> Command<Message>;
}

#[enum_dispatch(MessageHandler)]
#[derive(Debug, Clone)]
pub enum Message {
    Settings(SettingsMessage),
    Main(MainMessage),
    CloseSettings,
    ModelChanged,
    //
    RoomsWidget(RoomsWidgetMessage),
    PlaylistWidget(PlaylistWidgetMessage),
    MessagesWidget(MessagesWidgetMessage),
    DatabaseWidget(DatabaseWidgetMessage),
    FileSearchWidget(FileSearchWidgetMessage),
}

#[derive(Debug, Clone, Copy)]
pub struct CloseSettings;

impl MessageHandler for CloseSettings {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        model.close_settings();
        Command::none()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModelChanged;

impl MessageHandler for ModelChanged {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        model.update_from_inner_model();
        Command::none()
    }
}
