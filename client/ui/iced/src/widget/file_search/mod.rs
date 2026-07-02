use std::time::Instant;

use iced::widget::{
    Button, Column, Container, Id, Row, Scrollable, Text, TextInput, rich_text, span,
};
use iced::{Element, Length};
use itertools::Itertools;
use niketsu_core::file_database::FileEntry;
use niketsu_core::util::FuzzyResult;

use self::message::{Activate, Click, Close, FileSearchWidgetMessage, Input, Insert};
use crate::message::Message;
use crate::styling::{FileButton, ModalContainer};

pub mod message;

pub fn open_button() -> Element<'static, Message> {
    Element::from(
        Button::new(
            Text::new("Files")
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press(FileSearchWidgetMessage::from(Activate).into())
        .width(Length::Fill)
        .style(iced::widget::button::success),
    )
}

pub fn view(state: &FileSearchWidgetState) -> Element<'_, Message> {
    let mut results = vec![];
    for (index, file) in state.results.iter().enumerate() {
        let pressed = index == state.cursor_index;
        // TODO add modified date
        let text = file
            .entry
            .file_name()
            .chars()
            .enumerate()
            .chunk_by(|(i, _)| file.hits.contains(i))
            .into_iter()
            .map(|(bold, chars)| {
                let mut span: iced::widget::text::Span<'_, String> =
                    span(String::from_iter(chars.map(|(_, c)| c)));
                if bold {
                    span = span.underline(true);
                }
                span
            })
            .collect::<Vec<_>>();
        let row = Row::new().push(rich_text(text).width(Length::Fill));
        results.push(
            Button::new(Container::new(row).padding(2))
                .padding(0)
                .width(Length::Fill)
                .on_press(Click { index }.into())
                .style(FileButton::theme(pressed, true))
                .into(),
        );
    }
    let results = Column::with_children(results).width(Length::Fill);
    let input = TextInput::new("Search Query", &state.query)
        .on_input(|query| Input { query }.into())
        .on_submit(
            Insert {
                index: state.cursor_index,
            }
            .into(),
        )
        .id(iced::widget::Id::new("file_search_query"))
        .width(Length::Fill);
    let close_button = Button::new("Close")
        .on_press(Close.into())
        .style(iced::widget::button::danger);
    let top_row = Row::new().push(input).push(close_button).spacing(5);
    let mut base = Column::new().push(top_row).padding(5);
    if !state.results.is_empty() {
        base = base
            // TODO scroll with selection
            .push(Scrollable::new(results).id(Id::new("search")))
            .spacing(5);
    }

    let base: Element<'_, FileSearchWidgetMessage> =
        Container::new(base).style(ModalContainer::theme).into();
    base.map(Message::from)
}

#[derive(Debug, Default)]
pub struct FileSearchWidgetState {
    query: String,
    results: Vec<FuzzyResult<FileEntry>>,
    cursor_index: usize,
    last_click: Option<Instant>,
    active: bool,
}

impl FileSearchWidgetState {
    pub fn is_active(&self) -> bool {
        self.active
    }
}
