use iced::widget::scrollable::Id;
use iced::widget::{Button, Column, Container, Row, Scrollable, Text};
use iced::{Element, Length};
use niketsu_core::ui::UiModel;

use self::message::{MainMessage, MainMessageTrait, ReadyButton};
use super::message::Message;
use super::view::{SubWindowTrait, ViewModel};
use super::widget::database::DatabaseWidget;
use super::widget::messages::MessagesWidget;
use super::widget::playlist::PlaylistWidget;
use super::widget::rooms::RoomsWidget;
use crate::styling::{ContainerBorder, ResultButton};
use crate::widget::file_search::FileSearchWidget;

pub(super) mod message;

const SPACING: u16 = 5;

#[derive(Debug)]
pub struct MainView;

impl Default for MainView {
    fn default() -> Self {
        Self
    }
}

impl MainView {}

impl SubWindowTrait for MainView {
    type SubMessage = MainMessage;

    fn view<'a>(&'a self, view_model: &'a ViewModel) -> Element<Message> {
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
        btn = btn.on_press(MainMessage::from(ReadyButton).into());

        Row::new()
            .push(
                Column::new()
                    .push(FileSearchWidget::new(
                        view_model.get_file_search_widget_state(),
                    ))
                    .push(MessagesWidget::new(view_model.get_messages_widget_state()))
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

    fn update(&mut self, message: MainMessage, model: &UiModel) {
        message.handle(self, model);
    }
}
