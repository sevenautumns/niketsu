use std::sync::Arc;

use iced::widget::scrollable::RelativeOffset;
use iced::widget::{Button, Column, Container, Id, Row, Scrollable, Text, TextInput};
use iced::{Element, Length, Task};
use niketsu_core::ui::{MessageSource, PlayerMessage};

use self::message::{MessageInput, ScrollMessages, SendMessage};
use crate::RingBuffer;
use crate::message::Message;
use crate::styling::{ContainerBorder, MessageColor};

pub mod message;

const SPACING: f32 = 5.0;

pub fn view(state: &ChatWidgetState) -> Element<'_, Message> {
    let msgs: Vec<_> = state.messages.iter().map(|m| m.to_text()).collect();
    let messages = Container::new(
        Scrollable::new(Column::with_children(msgs))
            .width(Length::Fill)
            .on_scroll(|o| ScrollMessages(o.relative_offset()).into())
            .id(Id::new("messages")),
    )
    .style(ContainerBorder::theme)
    .padding(5.0)
    .width(Length::Fill)
    .height(Length::Fill);

    let message_input = Row::new()
        .push(
            TextInput::new("Message", &state.message)
                .width(Length::Fill)
                .on_input(|i| MessageInput(i).into())
                .on_submit(SendMessage.into()),
        )
        .push(Button::new("Send").on_press(SendMessage.into()))
        .spacing(SPACING);

    Element::from(
        Column::new()
            .push(messages)
            .push(message_input)
            .spacing(SPACING)
            .width(Length::Fill),
    )
    .map(Message::from)
}

#[derive(Debug, Clone)]
pub struct ChatWidgetState {
    messages: Arc<RingBuffer<PlayerMessage>>,
    message: String,
    offset: RelativeOffset,
}

impl Default for ChatWidgetState {
    fn default() -> Self {
        let offset = RelativeOffset {
            y: 1.0,
            ..Default::default()
        };
        Self {
            messages: Default::default(),
            message: Default::default(),
            offset,
        }
    }
}

impl ChatWidgetState {
    pub fn replace_messages(&mut self, messages: Arc<RingBuffer<PlayerMessage>>) {
        self.messages = messages;
    }

    pub fn snap(&self) -> Task<Message> {
        if self.offset.y == 1.0 {
            return iced::widget::operation::snap_to(Id::new("messages"), self.offset);
        }
        Task::none()
    }
}

trait PlayerMessageExt {
    fn to_text<'a>(&self) -> Element<'a, message::ChatWidgetMessage>;
}

impl PlayerMessageExt for PlayerMessage {
    fn to_text<'a>(&self) -> Element<'a, message::ChatWidgetMessage> {
        let when = self.timestamp.format("[%H:%M:%S]").to_string();
        let message = &self.message;

        let text = match &self.source {
            MessageSource::UserMessage(usr) => format!("{when} {usr}: {message}"),
            MessageSource::Server => format!("{when} {message}"),
            MessageSource::Internal => format!("{when} {message}"),
            MessageSource::UserAction(_) => format!("{when} {message}"),
        };

        Container::new(Text::new(text).style(MessageColor::theme(self.level)))
            .width(Length::Fill)
            .into()
    }
}
