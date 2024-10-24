use enum_dispatch::enum_dispatch;
use iced::Task;
use niketsu_core::ui::UiModel;

use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait MainMessageTrait {
    fn handle(self, model: &UiModel);
}

#[enum_dispatch(MainMessageTrait)]
#[derive(Debug, Clone)]
pub enum MainMessage {
    ReadyButton,
    StopDbUpdate,
    StartDbUpdate,
}

impl MessageHandler for MainMessage {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        MainMessageTrait::handle(self, &model.model);
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct ReadyButton;

impl MainMessageTrait for ReadyButton {
    fn handle(self, model: &UiModel) {
        model.user_ready_toggle();
    }
}

#[derive(Debug, Clone)]
pub struct StopDbUpdate;

impl MainMessageTrait for StopDbUpdate {
    fn handle(self, model: &UiModel) {
        model.stop_db_update();
    }
}

// Move to own widget
#[derive(Debug, Clone)]
pub struct StartDbUpdate;

impl MainMessageTrait for StartDbUpdate {
    fn handle(self, model: &UiModel) {
        model.start_db_update();
    }
}
