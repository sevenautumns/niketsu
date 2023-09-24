use dyn_clone::DynClone;
use iced::widget::scrollable::RelativeOffset;

use super::MessagesWidgetState;
use crate::message::Message;

pub trait MessagesWidgetMessage: std::fmt::Debug + DynClone + std::marker::Send {
    fn handle(self: Box<Self>, state: &mut MessagesWidgetState);
}

dyn_clone::clone_trait_object!(MessagesWidgetMessage);

#[derive(Debug, Clone)]
pub struct ScrollMessages(pub RelativeOffset);

impl MessagesWidgetMessage for ScrollMessages {
    fn handle(self: Box<Self>, state: &mut MessagesWidgetState) {
        state.offset = self.0;
    }
}

impl From<ScrollMessages> for Message {
    fn from(value: ScrollMessages) -> Self {
        Message::MessagesWidget(Box::new(value))
    }
}
