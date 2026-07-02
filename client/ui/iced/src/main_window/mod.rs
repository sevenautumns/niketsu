use iced::widget::{Button, Column, Container, Id, Row, Scrollable, Text};
use iced::{Element, Length};

use self::message::{MainMessage, ReadyButton};
use super::message::Message;
use super::view::ViewModel;
use super::widget::playlist::PlaylistWidget;
use super::widget::{chat, database, file_search, rooms, settings};
use crate::main_window::message::ShareButton;
use crate::styling::ContainerBorder;

pub(super) mod message;

const SPACING: f32 = 5.0;

pub fn view(view_model: &ViewModel) -> Element<'_, Message> {
    let ready_btn = match view_model.user().ready {
        true => Button::new(
            Text::new("Ready")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .style(iced::widget::button::success),
        false => Button::new(
            Text::new("Not Ready")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .style(iced::widget::button::danger),
    }
    .on_press(MainMessage::from(ReadyButton).into());

    let share_btn = match view_model.is_sharing() {
        true => Button::new(
            Text::new("sharing")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .style(iced::widget::button::success),
        false => Button::new(
            Text::new("Not sharing")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .style(iced::widget::button::danger),
    }
    .on_press(MainMessage::from(ShareButton).into());

    Row::new()
        .push(
            Column::new()
                .push(
                    Row::new()
                        .push(settings::open_button())
                        .push(file_search::open_button())
                        .spacing(SPACING),
                )
                .push(chat::view(&view_model.chat_widget_state))
                .spacing(SPACING)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .push(
            Column::new()
                .push(database::view(&view_model.database_widget_state))
                .push(
                    Container::new(
                        Column::new()
                            .push(if view_model.is_host() {
                                Text::new("HOST").style(iced::widget::text::success)
                            } else {
                                Text::new("CLIENT").style(iced::widget::text::primary)
                            })
                            .push(rooms::view(
                                &view_model.users_widget_state,
                                &view_model.user(),
                                view_model.is_host(),
                            ))
                            .width(Length::Fill)
                            .height(Length::Fill),
                    )
                    .style(ContainerBorder::theme)
                    .padding(SPACING)
                    .width(Length::Fill)
                    .height(Length::Fill),
                )
                .push(
                    Container::new(
                        Scrollable::new(PlaylistWidget::new(
                            &view_model.playlist_widget_state,
                            view_model.playing_video(),
                        ))
                        .width(Length::Fill)
                        .id(Id::new("playlist")),
                    )
                    .style(ContainerBorder::theme)
                    .padding(SPACING)
                    .height(Length::Fill),
                )
                .push(
                    Row::new()
                        .push(ready_btn.width(Length::FillPortion(2)))
                        .push(share_btn.width(Length::FillPortion(1)))
                        .spacing(SPACING),
                )
                .width(Length::Fill)
                .spacing(SPACING),
        )
        .spacing(SPACING)
        .padding(SPACING)
        .into()
}
