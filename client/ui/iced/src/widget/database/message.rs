use dyn_clone::DynClone;

use crate::message::Message;
use crate::UiModel;

pub trait DatabaseWidgetMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, model: &UiModel);
}

dyn_clone::clone_trait_object!(DatabaseWidgetMessage);

#[derive(Debug, Clone, Copy)]
pub struct StartDbUpdate;

impl DatabaseWidgetMessage for StartDbUpdate {
    fn handle(self: Box<Self>, model: &UiModel) {
        model.start_db_update();
    }
}

impl From<StartDbUpdate> for Message {
    fn from(value: StartDbUpdate) -> Self {
        Message::DatabaseWidget(Box::new(value))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StopDbUpdate;

impl DatabaseWidgetMessage for StopDbUpdate {
    fn handle(self: Box<Self>, model: &UiModel) {
        model.stop_db_update()
    }
}

impl From<StopDbUpdate> for Message {
    fn from(value: StopDbUpdate) -> Self {
        Message::DatabaseWidget(Box::new(value))
    }
}
