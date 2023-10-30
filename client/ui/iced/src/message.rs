use enum_dispatch::enum_dispatch;
use iced::keyboard::KeyCode;
use iced::{Command, Event};

use super::main_window::message::MainMessage;
use super::widget::chat::message::ChatWidgetMessage;
use super::widget::database::message::DatabaseWidgetMessage;
use super::widget::playlist::message::PlaylistWidgetMessage;
use super::widget::rooms::message::RoomsWidgetMessage;
use crate::view::ViewModel;
use crate::widget::file_search::message::FileSearchWidgetMessage;
use crate::widget::settings::message::SettingsWidgetMessage;

#[enum_dispatch]
pub trait MessageHandler {
    fn handle(self, model: &mut ViewModel) -> Command<Message>;
}

#[enum_dispatch(MessageHandler)]
#[derive(Debug, Clone)]
pub enum Message {
    Main(MainMessage),
    ModelChanged,
    EventOccured,
    //
    SettingsWidget(SettingsWidgetMessage),
    RoomsWidget(RoomsWidgetMessage),
    PlaylistWidget(PlaylistWidgetMessage),
    ChatWidget(ChatWidgetMessage),
    DatabaseWidget(DatabaseWidgetMessage),
    FileSearchWidget(FileSearchWidgetMessage),
}

#[derive(Debug, Clone, Copy)]
pub struct ModelChanged;

impl MessageHandler for ModelChanged {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        model.update_from_inner_model();
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct EventOccured(pub Event);

impl MessageHandler for EventOccured {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key_code,
            modifiers: _,
        }) = self.0
        {
            if key_code == KeyCode::Space {
                model.model.user_ready_toggle();
            }
        }
        Command::none()
    }
}
