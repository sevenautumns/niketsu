use enum_dispatch::enum_dispatch;
use iced::Command;

use super::MainView;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;
use crate::UiModel;

#[enum_dispatch]
pub trait MainMessageTrait {
    fn handle(self, ui: &mut MainView, model: &UiModel);
}

#[enum_dispatch(MainMessageTrait)]
#[derive(Debug, Clone)]
pub enum MainMessage {
    ReadyButton,
    StopDbUpdate,
    StartDbUpdate,
}

impl MessageHandler for MainMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        MainMessageTrait::handle(self, &mut model.main, &model.model);
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct ReadyButton;

impl MainMessageTrait for ReadyButton {
    fn handle(self, _: &mut MainView, model: &UiModel) {
        model.user_ready_toggle();
    }
}

#[derive(Debug, Clone)]
pub struct StopDbUpdate;

impl MainMessageTrait for StopDbUpdate {
    fn handle(self, _: &mut MainView, model: &UiModel) {
        model.stop_db_update();
    }
}

// Move to own widget
#[derive(Debug, Clone)]
pub struct StartDbUpdate;

impl MainMessageTrait for StartDbUpdate {
    fn handle(self, _: &mut MainView, model: &UiModel) {
        model.start_db_update();
    }
}
