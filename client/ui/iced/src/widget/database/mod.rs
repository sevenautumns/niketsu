use iced::widget::{Button, Container, ProgressBar, Row, Text, Tooltip};
use iced::{Element, Length};
use niketsu_core::file_database::FileStore;

use self::message::{DatabaseWidgetMessage, StartDbUpdate, StopDbUpdate};
use crate::TEXT_SIZE;
use crate::message::Message;
use crate::styling::{ContainerBorder, FileButton, FileProgressBar};

pub mod message;

pub fn view(state: &DatabaseWidgetState) -> Element<'_, Message> {
    let finished = 1.0 == state.ratio;
    let main: Element<_, _> = match finished {
        true => {
            let len = state.database.len();
            Container::new(
                Button::new(Text::new(format!("{len} files in database")))
                    .style(FileButton::theme(false, true)),
            )
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .style(ContainerBorder::theme)
            .width(Length::Fill)
            .into()
        }
        false => ProgressBar::new(0.0..=1.0, state.ratio)
            .style(FileProgressBar::theme(finished))
            // Text size + 2 times default button padding
            .girth(Length::Fixed(TEXT_SIZE + 16.0))
            .into(),
    };

    let update_msg = match finished {
        true => StartDbUpdate.into(),
        false => StopDbUpdate.into(),
    };
    let update_btn = match finished {
        true => Button::new("Update"),
        false => Button::new("Stop"),
    }
    .on_press(update_msg)
    .style(move |theme, status| {
        if finished {
            iced::widget::button::success(theme, status)
        } else {
            iced::widget::button::danger(theme, status)
        }
    });

    let update_text = match finished {
        true => "Update file database",
        false => "Stop update of file database",
    };
    let update_tooltip: Element<_, _> = Tooltip::new(
        update_btn,
        update_text,
        iced::widget::tooltip::Position::Bottom,
    )
    .into();

    let base: Element<'_, DatabaseWidgetMessage> = Row::new()
        .push(main)
        .push(update_tooltip)
        .spacing(5.0)
        .into();
    base.map(Message::from)
}

#[derive(Debug, Clone, Default)]
pub struct DatabaseWidgetState {
    database: FileStore,
    ratio: f32,
}

impl DatabaseWidgetState {
    pub fn update_file_store(&mut self, store: FileStore) {
        self.database = store;
    }

    pub fn update_progress(&mut self, ratio: f32) {
        self.ratio = ratio;
    }
}
