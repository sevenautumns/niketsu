use std::sync::Arc;

use iced::advanced::widget::Tree;
use iced::event::Status;
use iced::mouse::Cursor;
use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{Button, Column, Container, Row, Scrollable, Text, TextInput};
use iced::{Command, Element, Length, Rectangle, Renderer, Theme};
use niketsu_core::ui::{MessageSource, PlayerMessage};

use self::message::{ChatWidgetMessage, MessageInput, ScrollMessages, SendMessage};
use crate::message::Message;
use crate::styling::{ContainerBorder, MessageColor};
use crate::RingBuffer;

pub mod message;

const SPACING: u16 = 5;

pub struct ChatWidget<'a> {
    base: Element<'a, Message>,
}

impl<'a> ChatWidget<'a> {
    pub fn new(state: &ChatWidgetState) -> Self {
        let mut column = Column::new()
            .spacing(SPACING)
            .width(Length::Fill)
            .width(Length::Fill);

        let msgs: Vec<_> = state.messages.iter().map(|m| m.to_text()).collect();
        let messages = Container::new(
            Scrollable::new(Column::with_children(msgs))
                .width(Length::Fill)
                .on_scroll(|o| ChatWidgetMessage::from(ScrollMessages(o.relative_offset())).into())
                .id(Id::new("messages")),
        )
        .style(ContainerBorder::basic())
        .padding(5.0)
        .width(Length::Fill)
        .height(Length::Fill);
        column = column.push(messages);

        let message_input = Row::new()
            .push(
                TextInput::new("Message", &state.message)
                    .width(Length::Fill)
                    .on_input(|i| ChatWidgetMessage::from(MessageInput(i)).into())
                    .on_submit(ChatWidgetMessage::from(SendMessage).into()),
            )
            .push(Button::new("Send").on_press(ChatWidgetMessage::from(SendMessage).into()))
            .spacing(SPACING);
        column = column.push(message_input);

        let base = column.into();
        Self { base }
    }
}

impl<'a> iced::advanced::Widget<Message, Theme, Renderer> for ChatWidget<'a> {
    fn size(&self) -> iced::Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn on_event(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        self.base.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }
}

impl<'a> From<ChatWidget<'a>> for Element<'a, Message> {
    fn from(msgs: ChatWidget<'a>) -> Self {
        Self::new(msgs)
    }
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

    pub fn snap(&self) -> Command<Message> {
        if self.offset.y == 1.0 {
            return iced::widget::scrollable::snap_to(Id::new("messages"), self.offset);
        }
        Command::none()
    }
}

pub trait PlayerMessageExt {
    fn to_text<'a>(&self) -> Element<'a, Message>;
}

impl PlayerMessageExt for PlayerMessage {
    fn to_text<'a>(&self) -> Element<'a, Message> {
        let when = self.timestamp.format("[%H:%M:%S]").to_string();
        let message = &self.message;

        let text = match &self.source {
            MessageSource::UserMessage(usr) => format!("{when} {usr}: {message}"),
            MessageSource::Server => format!("{when} {message}"),
            MessageSource::Internal => format!("{when} {message}"),
            MessageSource::UserAction(_) => format!("{when} {message}"),
        };

        Container::new(Text::new(text))
            .style(MessageColor::theme(self.level))
            .width(Length::Fill)
            .into()
    }
}
