use arcstr::ArcStr;
use iced::widget::{Button, Column, Container, Row, Text};
use iced::{Element, Length};

use self::message::{Close, Handover, UserActionsWidgetMessage};
use crate::message::Message;
use crate::styling::ModalContainer;

pub mod message;

pub fn view(state: &UserActionsWidgetState) -> Element<'_, Message> {
    let user = state.user.as_deref().unwrap_or_default();
    let handover_button = Button::new(
        Text::new("Hand over host")
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .on_press(Handover.into())
    .width(Length::Fill)
    .style(iced::widget::button::danger);
    let close_button = Button::new("Close")
        .on_press(Close.into())
        .style(iced::widget::button::secondary);
    let top_row = Row::new()
        .push(Text::new(user).width(Length::Fill))
        .push(close_button)
        .spacing(5);
    let base = Column::new()
        .push(top_row)
        .push(handover_button)
        .spacing(5)
        .padding(5)
        .width(Length::Fixed(250.0));

    let base: Element<'_, UserActionsWidgetMessage> =
        Container::new(base).style(ModalContainer::theme).into();
    base.map(Message::from)
}

#[derive(Debug, Default)]
pub struct UserActionsWidgetState {
    user: Option<ArcStr>,
}

impl UserActionsWidgetState {
    pub fn is_active(&self) -> bool {
        self.user.is_some()
    }
}
