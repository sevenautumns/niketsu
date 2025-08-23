use niketsu_core::fuzzy::FuzzyEntry;
use niketsu_core::util::FuzzyResult;
use ratatui::buffer::Buffer;
use ratatui::prelude::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, ListItem, ListState, Widget};
use tui_textarea::{Input, TextArea};

pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod command;
pub(crate) mod database;
pub(crate) mod footer;
pub(crate) mod help;
pub(crate) mod login;
pub(crate) mod media;
pub(crate) mod nav;
pub(crate) mod options;
pub(crate) mod playlist;
pub(crate) mod playlist_browser;
pub(crate) mod recently;
pub(crate) mod search;
pub(crate) mod users;

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
        let index = match self.inner.selected() {
            Some(i) => {
                if i == 0 {
                    len.saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.inner.select(Some(index));
    }

    fn jump_next(&mut self, offset: usize) {
        if let Some(i) = self.selected() {
            self.inner.select(Some(i.saturating_sub(offset)));
        }
    }

    fn overflowing_previous(&mut self, len: usize) {
        let index = match self.inner.selected() {
            Some(i) => {
                if i >= len.saturating_sub(1) {
                    0
                } else {
                    i.saturating_add(1)
                }
            }
            None => 0,
        };
        self.inner.select(Some(index));
    }

    fn limited_previous(&mut self, len: usize) {
        let index = match self.inner.selected() {
            Some(i) => {
                if i >= len.saturating_sub(1) {
                    len.saturating_sub(1)
                } else {
                    i.saturating_add(1)
                }
            }
            None => 0,
        };
        self.inner.select(Some(index));
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
}

pub struct TextAreaWrapper {
    inner: TextArea<'static>,
    title: Option<String>,
}

impl std::fmt::Debug for TextAreaWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.lines().join("").as_str())
    }
}

impl Default for TextAreaWrapper {
    fn default() -> Self {
        let mut wrapper = Self {
            inner: TextArea::default(),
            title: Default::default(),
        };
        wrapper.with_default_style();
        wrapper
    }
}

impl Clone for TextAreaWrapper {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            title: self.title.clone(),
        }
    }
}

impl From<(String, String)> for TextAreaWrapper {
    fn from(value: (String, String)) -> Self {
        let mut text_area = TextAreaWrapper {
            inner: TextArea::new(vec![value.1]),
            title: Some(value.0),
        };
        text_area.with_block_style(Style::default());
        text_area
    }
}

impl TextAreaWrapper {
    fn new(title: String, content: String) -> Self {
        Self::from((title.clone(), content))
    }

    fn highlight(&mut self, block_style: Style, cursor_style: Style) {
        self.inner.set_tab_length(2);
        self.inner.set_style(Style::default().gray());
        self.inner.set_cursor_style(cursor_style);
        self.inner.set_cursor_line_style(Style::default());
        self.with_block_style(block_style);
    }

    fn with_default_style(&mut self) -> &mut Self {
        self.inner.set_tab_length(2);
        self.inner
            .set_style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM));
        self.inner.set_cursor_line_style(Style::default());
        self.inner.set_cursor_style(Style::default());
        self.with_block_style(Style::default().gray());
        self
    }

    fn with_block_style(&mut self, style: Style) {
        let block = self.inner.block();
        let title = self.title.clone();

        match block {
            Some(b) => self.inner.set_block(b.clone().style(style)),
            _ => {
                let b = match title {
                    Some(t) => Block::default().title(t).borders(Borders::ALL).style(style),
                    _ => Block::default().borders(Borders::ALL).style(style),
                };
                self.inner.set_block(b);
            }
        }
    }

    fn with_block(&mut self, block: Block<'static>) -> &mut Self {
        match self.title.clone() {
            Some(t) => self.inner.set_block(block.title(t)),
            _ => self.inner.set_block(block),
        }
        self
    }

    fn with_placeholder(&mut self, placeholder_text: &str) -> &mut Self {
        self.inner.set_placeholder_text(placeholder_text);
        self
    }

    fn with_mask(&mut self, placeholder_text: &str) -> &mut Self {
        self.inner.set_placeholder_text(placeholder_text);
        self.inner.set_mask_char('\u{2022}');
        self
    }

    fn get_input(&self) -> String {
        self.inner.lines().join("")
    }

    fn lines(&self) -> &[String] {
        self.inner.lines()
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.inner.render(area, buf)
    }

    fn input(&mut self, input: impl Into<Input>) -> bool {
        self.inner.input(input)
    }
}

fn color_hits<E>(result: &FuzzyResult<E>, color: Option<Color>) -> ListItem
where
    E: FuzzyEntry,
{
    let mut text = Vec::new();
    let name = result.entry.key();
    let hits = &result.hits;
    let mut hits_index = 0;
    let hits_len = hits.len();
    for (index, char) in name.char_indices() {
        if hits_index < hits_len && index == hits[hits_index] {
            text.push(Span::styled(
                char.to_string(),
                Style::default().fg(color.unwrap_or(Color::Yellow)),
            ));
            hits_index += 1;
        } else {
            text.push(Span::styled(
                char.to_string(),
                Style::default().fg(color.unwrap_or(Color::Gray)),
            ));
        }
    }
    ListItem::new(Line::from(text))
}
