use enum_dispatch::enum_dispatch;
use iced::widget::scrollable::RelativeOffset;
use iced::Command;
use niketsu_core::ui::UiModel;

use super::ChatWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait ChatWidgetMessageTrait {
    fn handle(self, state: &mut ChatWidgetState, model: &UiModel) -> Command<Message>;
}

#[enum_dispatch(ChatWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum ChatWidgetMessage {
    ScrollMessages,
    MessageInput,
    SendMessage,
}

impl MessageHandler for ChatWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Command<Message> {
        ChatWidgetMessageTrait::handle(self, &mut model.chat_widget_statet, &model.model)
    }
}

#[derive(Debug, Clone)]
pub struct ScrollMessages(pub RelativeOffset);

impl ChatWidgetMessageTrait for ScrollMessages {
    fn handle(self, _: &mut ChatWidgetState, _: &UiModel) -> Command<Message> {
        // TODO how to snap to the end?
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct MessageInput(pub String);

impl ChatWidgetMessageTrait for MessageInput {
    fn handle(self, state: &mut ChatWidgetState, _: &UiModel) -> Command<Message> {
        state.message = self.0;
        Command::none()
    }
}

#[derive(Debug, Clone)]
pub struct SendMessage;

impl ChatWidgetMessageTrait for SendMessage {
    fn handle(self, state: &mut ChatWidgetState, model: &UiModel) -> Command<Message> {
        let message = state.message.clone();
        if message.is_empty() {
            return Command::none();
        }
        state.message = String::new();
        model.send_message(message);
        Command::none()
    }
}
