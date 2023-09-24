use iced::widget::scrollable::{Id, RelativeOffset};
use iced::widget::{Button, Column, Container, Row, Scrollable, Text, TextInput};
use iced::{Element, Length};

use self::message::{MainMessage, MessageInput, ReadyButton, SendMessage};
use super::message::Message;
use super::view::{SubWindowTrait, ViewModel};
use super::widget::database::DatabaseWidget;
use super::widget::messages::MessagesWidget;
use super::widget::playlist::PlaylistWidget;
use super::widget::rooms::RoomsWidget;
use super::UiModel;
use crate::styling::{ContainerBorder, ResultButton};

pub(super) mod message;

const SPACING: u16 = 5;

#[derive(Debug)]
pub struct MainView {
    message: String,
    messages_scroll: RelativeOffset,
}

impl Default for MainView {
    fn default() -> Self {
        Self {
            message: Default::default(),
            messages_scroll: RelativeOffset::END,
        }
    }
}

impl MainView {}

impl SubWindowTrait for MainView {
    type SubMessage = Box<dyn MainMessage>;

    fn view(&self, view_model: &ViewModel) -> Element<Message> {
        let mut btn: Button<Message>;
        match view_model.user().ready {
            true => {
                btn = Button::new(
                    Text::new("Ready")
                        .width(Length::Fill)
                        .horizontal_alignment(iced::alignment::Horizontal::Center),
                )
                .style(ResultButton::ready())
            }
            false => {
                btn = Button::new(
                    Text::new("Not Ready")
                        .width(Length::Fill)
                        .horizontal_alignment(iced::alignment::Horizontal::Center),
                )
                .style(ResultButton::not_ready())
            }
        }
        btn = btn.on_press(ReadyButton.into());

        Row::new()
            .push(
                Column::new()
                    .push(MessagesWidget::new(view_model.get_messages_widget_state()))
                    .push(
                        Row::new()
                            .push(
                                TextInput::new("Message", &self.message)
                                    .width(Length::Fill)
                                    .on_input(|i| MessageInput(i).into())
                                    .on_submit(SendMessage.into()),
                            )
                            .push(Button::new("Send").on_press(SendMessage.into()))
                            .spacing(SPACING),
                    )
                    .spacing(SPACING)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .push(
                Column::new()
                    .push(DatabaseWidget::new(view_model.get_database_widget_state()))
                    .push(
                        Container::new(RoomsWidget::new(
                            view_model.get_rooms_widget_state(),
                            &view_model.user(),
                            &view_model.theme(),
                        ))
                        .style(ContainerBorder::basic())
                        .padding(SPACING)
                        .width(Length::Fill)
                        .height(Length::Fill),
                    )
                    .push(
                        Container::new(
                            Scrollable::new(PlaylistWidget::new(
                                view_model.get_playlist_widget_state().clone(),
                                view_model.playing_video(),
                            ))
                            .width(Length::Fill)
                            .id(Id::new("playlist")),
                        )
                        .style(ContainerBorder::basic())
                        .padding(SPACING)
                        .height(Length::Fill),
                    )
                    .push(btn.width(Length::Fill))
                    .width(Length::Fill)
                    .spacing(SPACING),
            )
            .spacing(SPACING)
            .padding(SPACING)
            .into()
    }

    fn update(&mut self, message: Box<dyn MainMessage>, model: &UiModel) {
        message.handle(self, model);
    }
}
