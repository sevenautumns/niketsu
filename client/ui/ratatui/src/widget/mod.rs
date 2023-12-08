use ratatui::prelude::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, ListState, Widget};
use ratatui_textarea::{CursorMove, Input, TextArea};

pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod fuzzy_search;
pub(crate) mod help;
pub(crate) mod login;
pub(crate) mod media;
pub(crate) mod options;
pub(crate) mod playlist;
pub(crate) mod room;

pub trait OverlayWidgetState {
    fn area(&self, r: Rect) -> Rect;
}

#[derive(Debug, Default, Clone)]
pub struct ListStateWrapper {
    inner: ListState,
}

impl ListStateWrapper {
    fn next(&mut self) {
        let i = match self.inner.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.inner.select(Some(i));
    }

    fn overflowing_next(&mut self, len: usize) {
        let i = match self.inner.selected() {
            Some(i) => {
                if i == 0 {
                    len.saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.inner.select(Some(i));
    }

    fn jump_next(&mut self, offset: usize) {
        if let Some(i) = self.selected() {
            self.inner.select(Some(i.saturating_sub(offset)));
        }
    }

    fn overflowing_previous(&mut self, len: usize) {
        let i = match self.inner.selected() {
            Some(i) => {
                if i >= len.saturating_sub(1) {
                    0
                } else {
                    i.saturating_add(1)
                }
            }
            None => 0,
        };
        self.inner.select(Some(i));
    }

    fn limited_previous(&mut self, len: usize) {
        let i = match self.inner.selected() {
            Some(i) => {
                if i >= len.saturating_sub(1) {
                    len.saturating_sub(1)
                } else {
                    i.saturating_add(1)
                }
            }
            None => 0,
        };
        self.inner.select(Some(i));
    }

    fn limited_jump_previous(&mut self, offset: usize, len: usize) {
        if let Some(i) = self.selected() {
            let jump_index = usize::min(i.saturating_add(offset), len.saturating_sub(1));
            self.inner.select(Some(jump_index));
        }
    }

    fn limit(&mut self, len: usize) {
        if let Some(i) = self.selected() {
            let mindex = usize::min(i, len);
            self.inner.select(Some(mindex));
        }
    }

    fn select(&mut self, index: Option<usize>) {
        self.inner.select(index);
    }

    fn selected(&self) -> Option<usize> {
        self.inner.selected()
    }

    fn inner(&mut self) -> &mut ListState {
        &mut self.inner
    }

    fn clone_inner(&self) -> ListState {
        self.inner.clone()
    }
}

pub struct TextAreaWrapper {
    inner: TextArea<'static>,
}

impl std::fmt::Debug for TextAreaWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.lines().join("").as_str())
    }
}

impl Default for TextAreaWrapper {
    fn default() -> Self {
        let wrapper = Self {
            inner: TextArea::default(),
        };
        wrapper.set_default_stye()
    }
}

impl Clone for TextAreaWrapper {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl From<String> for TextAreaWrapper {
    fn from(value: String) -> Self {
        TextAreaWrapper {
            inner: TextArea::new(vec![value]),
        }
    }
}

impl TextAreaWrapper {
    fn new(title: &str, content: String) -> Self {
        let mut text_area = Self::from(content);
        text_area.inner.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string()),
        );
        text_area
    }

    fn set_textarea_style(&mut self, style: Style, cursor_style: Style) {
        self.inner.set_style(style);
        self.inner.set_cursor_style(cursor_style);
    }

    fn set_default_stye(self) -> Self {
        let mut text_area = self;
        text_area.inner.set_tab_length(2);
        text_area
            .inner
            .set_style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM));
        text_area.inner.set_cursor_line_style(Style::default());
        text_area.inner.set_cursor_style(Style::default());
        text_area
    }

    fn set_block(&mut self, block: Block<'static>) {
        self.inner.set_block(block);
    }

    fn into_masked(self, title: &str) -> Self {
        let lines = self.inner.lines();
        let masked_lines: String = lines
            .iter()
            .map(|l| "*".repeat(l.chars().count()))
            .collect::<Vec<String>>()
            .join("");
        let mut text_area = TextAreaWrapper::from(masked_lines);
        text_area.inner.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string()),
        );
        let cursor = self.inner.cursor();
        text_area = text_area.set_default_stye();
        text_area
            .inner
            .move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
        text_area
    }

    fn get_input(&self) -> String {
        self.inner.lines().join("")
    }

    fn lines(&self) -> &[String] {
        self.inner.lines()
    }

    fn widget(&self) -> impl Widget + '_ {
        self.inner.widget()
    }

    fn input(&mut self, input: impl Into<Input>) -> bool {
        self.inner.input(input)
    }
}
