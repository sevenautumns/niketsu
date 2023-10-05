use enum_dispatch::enum_dispatch;
use iced::widget::scrollable::RelativeOffset;
use iced::Command;
use niketsu_core::ui::UiModel;

use super::MessagesWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait MessagesWidgetMessageTrait {
    fn handle(self, state: &mut MessagesWidgetState, model: &UiModel) -> Command<Message>;
}

#[enum_dispatch(MessagesWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum MessagesWidgetMessage {
    ScrollMessages,
    MessageInput,
    SendMessage,
}

impl MessageHandler for MessagesWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        MessagesWidgetMessageTrait::handle(self, &mut model.messages_widget_state, &model.model)
    }
}

#[derive(Debug, Clone)]
pub struct ScrollMessages(pub RelativeOffset);

impl MessagesWidgetMessageTrait for ScrollMessages {
    fn handle(self, _: &mut MessagesWidgetState, _: &UiModel) -> Command<Message> {
        // TODO how to snap to the end?
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct MessageInput(pub String);

impl MessagesWidgetMessageTrait for MessageInput {
    fn handle(self, state: &mut MessagesWidgetState, _: &UiModel) -> Command<Message> {
        state.message = self.0;
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct SendMessage;

impl MessagesWidgetMessageTrait for SendMessage {
    fn handle(self, state: &mut MessagesWidgetState, model: &UiModel) -> Command<Message> {
        let message = state.message.clone();
        if message.is_empty() {
            return Command::none();
        }
        state.message = String::new();
        model.send_message(message);
        Command::none()
    }
}
