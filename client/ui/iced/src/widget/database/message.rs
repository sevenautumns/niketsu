use enum_dispatch::enum_dispatch;
use iced::Command;

use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;
use crate::UiModel;

#[enum_dispatch]
pub trait DatabaseWidgetMessageTrait {
    fn handle(self, model: &UiModel);
}

#[enum_dispatch(DatabaseWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum DatabaseWidgetMessage {
    StartDbUpdate,
    StopDbUpdate,
}

impl MessageHandler for DatabaseWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        DatabaseWidgetMessageTrait::handle(self, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StartDbUpdate;

impl DatabaseWidgetMessageTrait for StartDbUpdate {
    fn handle(self, model: &UiModel) {
        model.start_db_update();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StopDbUpdate;

impl DatabaseWidgetMessageTrait for StopDbUpdate {
    fn handle(self, model: &UiModel) {
        model.stop_db_update()
    }
}
