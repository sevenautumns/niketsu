use super::main_window::message::MainMessage;
use super::settings_window::message::SettingsMessage;
use super::widget::database::message::DatabaseWidgetMessage;
use super::widget::messages::message::MessagesWidgetMessage;
use super::widget::playlist::message::PlaylistWidgetMessage;
use super::widget::rooms::message::RoomsWidgetMessage;
use crate::widget::file_search::message::FileSearchWidgetMessage;

#[derive(Debug, Clone)]
pub enum Message {
    Settings(Box<dyn SettingsMessage>),
    Main(Box<dyn MainMessage>),
    // OpenSettings,
    CloseSettings,
    ModelChanged,
    ///
    RoomsWidget(Box<dyn RoomsWidgetMessage>),
    PlaylistWidget(Box<dyn PlaylistWidgetMessage>),
    MessagesWidget(Box<dyn MessagesWidgetMessage>),
    DatabaseWidget(Box<dyn DatabaseWidgetMessage>),
    FileSearchWidget(Box<dyn FileSearchWidgetMessage>),
}
