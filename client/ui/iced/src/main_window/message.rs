use dyn_clone::DynClone;
use iced::widget::scrollable::RelativeOffset;

use super::MainView;
use crate::message::Message;
use crate::UiModel;

pub trait MainMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, ui: &mut MainView, model: &UiModel);
}

dyn_clone::clone_trait_object!(MainMessage);

#[derive(Debug, Clone)]
pub struct ReadyButton;

impl MainMessage for ReadyButton {
    fn handle(self: Box<Self>, _: &mut MainView, model: &UiModel) {
        model.user_ready_toggle();
    }
}

impl From<ReadyButton> for Message {
    fn from(value: ReadyButton) -> Self {
        Message::Main(Box::new(value))
    }
}

// TODO move to own widget
#[derive(Debug, Clone)]
pub struct SendMessage;

impl MainMessage for SendMessage {
    fn handle(self: Box<Self>, ui: &mut MainView, model: &UiModel) {
        let message = ui.message.clone();
        if message.is_empty() {
            return;
        }
        ui.message = String::new();
        model.send_message(message)
    }
}

impl From<SendMessage> for Message {
    fn from(value: SendMessage) -> Self {
        Message::Main(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct StopDbUpdate;

impl MainMessage for StopDbUpdate {
    fn handle(self: Box<Self>, _: &mut MainView, model: &UiModel) {
        model.stop_db_update();
    }
}

impl From<StopDbUpdate> for Message {
    fn from(value: StopDbUpdate) -> Self {
        Message::Main(Box::new(value))
    }
}

// Move to own widget
#[derive(Debug, Clone)]
pub struct StartDbUpdate;

impl MainMessage for StartDbUpdate {
    fn handle(self: Box<Self>, _: &mut MainView, model: &UiModel) {
        model.start_db_update();
    }
}

impl From<StartDbUpdate> for Message {
    fn from(value: StartDbUpdate) -> Self {
        Message::Main(Box::new(value))
    }
}

#[derive(Debug, Clone)]
pub struct ScrollMessage(pub RelativeOffset);

impl MainMessage for ScrollMessage {
    fn handle(self: Box<Self>, ui: &mut MainView, _: &UiModel) {
        ui.messages_scroll = self.0;
    }
}

impl From<ScrollMessage> for Message {
    fn from(value: ScrollMessage) -> Self {
        Message::Main(Box::new(value))
    }
}
#[derive(Debug, Clone)]
pub struct MessageInput(pub String);

impl MainMessage for MessageInput {
    fn handle(self: Box<Self>, ui: &mut MainView, _: &UiModel) {
        ui.message = self.0;
    }
}

impl From<MessageInput> for Message {
    fn from(value: MessageInput) -> Self {
        Message::Main(Box::new(value))
    }
}
