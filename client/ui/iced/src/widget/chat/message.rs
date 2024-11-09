use enum_dispatch::enum_dispatch;
use iced::widget::scrollable::RelativeOffset;
use iced::Task;
use niketsu_core::ui::UiModel;

use super::ChatWidgetState;
use crate::message::{Message, MessageHandler};
use crate::view::ViewModel;

#[enum_dispatch]
pub trait ChatWidgetMessageTrait {
    fn handle(self, state: &mut ChatWidgetState, model: &UiModel) -> Task<Message>;
}

#[enum_dispatch(ChatWidgetMessageTrait)]
#[derive(Debug, Clone)]
pub enum ChatWidgetMessage {
    ScrollMessages,
    MessageInput,
    SendMessage,
}

impl MessageHandler for ChatWidgetMessage {
    fn handle(self, model: &mut ViewModel) -> Task<Message> {
        ChatWidgetMessageTrait::handle(self, &mut model.chat_widget_statet, &model.model)
    }
}

#[derive(Debug, Clone)]
pub struct ScrollMessages(pub RelativeOffset);

impl ChatWidgetMessageTrait for ScrollMessages {
    fn handle(self, state: &mut ChatWidgetState, _: &UiModel) -> Task<Message> {
        state.offset = self.0;
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct MessageInput(pub String);

impl ChatWidgetMessageTrait for MessageInput {
    fn handle(self, state: &mut ChatWidgetState, _: &UiModel) -> Task<Message> {
        state.message = self.0;
        Task::none()
    }
}

#[derive(Debug, Clone)]
pub struct SendMessage;

impl ChatWidgetMessageTrait for SendMessage {
    fn handle(self, state: &mut ChatWidgetState, model: &UiModel) -> Task<Message> {
        let message = state.message.clone();
        if message.is_empty() {
            return Task::none();
        }
        state.message = String::new();
        model.send_message(message);
        Task::none()
    }
}
