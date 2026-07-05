use iced::widget::text::Wrapping;
use iced::widget::{Button, Column, Container, Id, PaneGrid, Row, Scrollable, Text, pane_grid};
use iced::{Element, Length};

use self::message::{MainMessage, ReadyButton};
use super::message::Message;
use super::view::ViewModel;
use super::widget::playlist::PlaylistWidget;
use super::widget::{chat, database, file_search, rooms, settings};
use crate::main_window::message::{RequestButton, ShareButton};
use crate::message::PaneResized;
use crate::styling::ContainerBorder;

pub(super) mod message;

const SPACING: f32 = 5.0;
/// Extra grabbable space around the pane split, in pixels.
const RESIZE_LEEWAY: f32 = 10.0;

/// The two resizable halves of the main window.
#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    Chat,
    Controls,
}

pub fn view(view_model: &ViewModel) -> Element<'_, Message> {
    Container::new(
        PaneGrid::new(&view_model.panes, |_, kind, _| {
            pane_grid::Content::new(match kind {
                PaneKind::Chat => chat_pane(view_model),
                PaneKind::Controls => controls_pane(view_model),
            })
        })
        .on_resize(RESIZE_LEEWAY, |event| PaneResized(event).into())
        .spacing(SPACING),
    )
    .padding(SPACING)
    .into()
}

fn chat_pane(view_model: &ViewModel) -> Element<'_, Message> {
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
        .height(Length::Fill)
        .into()
}

/// A bottom-row button label: centered, never wrapping to a second
/// line, so the row keeps a single-line height at any pane width.
fn button_label(label: &str) -> Text<'_> {
    Text::new(label)
        .width(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .wrapping(Wrapping::None)
}

fn controls_pane(view_model: &ViewModel) -> Element<'_, Message> {
    let ready_btn = match view_model.user().ready {
        true => Button::new(button_label("Ready")).style(iced::widget::button::success),
        false => Button::new(button_label("Not Ready")).style(iced::widget::button::danger),
    }
    .clip(true)
    .on_press(MainMessage::from(ReadyButton).into());

    let share_btn = match view_model.is_sharing() {
        true => Button::new(button_label("sharing")).style(iced::widget::button::success),
        false => Button::new(button_label("Not sharing")).style(iced::widget::button::danger),
    }
    .clip(true)
    .on_press(MainMessage::from(ShareButton).into());

    let request_btn = Button::new(button_label("Request"))
        .clip(true)
        .on_press(MainMessage::from(RequestButton).into());

    Column::new()
        .push(database::view(&view_model.database_widget_state))
        .push(
            Container::new(
                Column::new()
                    .push(Text::new(view_model.users_widget_state.room_name()))
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
                .push(ready_btn.width(Length::FillPortion(4)))
                .push(share_btn.width(Length::FillPortion(1)))
                .push(request_btn.width(Length::FillPortion(1)))
                .spacing(SPACING),
        )
        .width(Length::Fill)
        .spacing(SPACING)
        .into()
}
