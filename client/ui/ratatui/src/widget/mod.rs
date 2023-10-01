use ratatui::prelude::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};
use ratatui_textarea::{CursorMove, Input, TextArea};

pub(crate) mod chat;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod fuzzy_search;
pub(crate) mod login;
pub(crate) mod options;
pub(crate) mod playlist;

pub trait OverlayWidget {
    fn area(&self, r: Rect) -> Rect;
}

pub struct TextAreaWrapper<'a> {
    inner: TextArea<'a>,
}

impl<'a> std::fmt::Debug for TextAreaWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.lines().join("").as_str())
    }
}

impl<'a> Default for TextAreaWrapper<'a> {
    fn default() -> Self {
        let mut wrapper = Self {
            inner: TextArea::default(),
        };
        wrapper.set_default_stye();
        wrapper
    }
}

impl<'a> Clone for TextAreaWrapper<'a> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<'a> From<String> for TextAreaWrapper<'a> {
    fn from(value: String) -> Self {
        TextAreaWrapper {
            inner: TextArea::new(vec![value]),
        }
    }
}

impl<'a> TextAreaWrapper<'a> {
    fn new(title: &'a str) -> Self {
        let mut text_area = Self::default();
        text_area
            .inner
            .set_block(Block::default().borders(Borders::ALL).title(title));
        text_area
    }

    fn set_textarea_style(&mut self, style: Style, cursor_style: Style) {
        self.inner.set_style(style);
        self.inner.set_cursor_style(cursor_style);
    }

    fn into_masked(self, title: &str) -> TextAreaWrapper<'_> {
        let lines = self.inner.lines();
        let masked_lines: String = lines
            .iter()
            .map(|l| "*".repeat(l.chars().count()))
            .collect::<Vec<String>>()
            .join("");
        let mut text_area = TextAreaWrapper::from(masked_lines);
        text_area
            .inner
            .set_block(Block::default().borders(Borders::ALL).title(title));
        text_area.set_default_stye();
        let cursor = self.inner.cursor();
        text_area
            .inner
            .move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
        text_area
    }

    fn set_default_stye(&mut self) {
        self.inner.set_tab_length(2);
        self.inner
            .set_style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM));
        self.inner.set_cursor_line_style(Style::default());
        self.inner.set_cursor_style(Style::default());
    }

    fn set_block(&mut self, block: Block<'a>) {
        self.inner.set_block(block);
    }

    fn lines(&'a self) -> &[String] {
        self.inner.lines()
    }

    fn widget(&'a self) -> impl Widget + 'a {
        self.inner.widget()
    }

    fn input(&mut self, input: impl Into<Input>) -> bool {
        self.inner.input(input)
    }
}
