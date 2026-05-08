use niketsu_core::fuzzy::FuzzyEntry;
use niketsu_core::util::FuzzyResult;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, ListItem, ListState, Widget};
use tui_textarea::{Input, TextArea};

use crate::theme::Theme;

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
pub(crate) mod settings;
pub(crate) mod users;

pub trait OverlayWidgetState {
    fn area(&self, r: Rect) -> Rect;
    fn default_area(&self, r: Rect) -> Rect {
        let height = self.default_height(r);
        let width = self.default_width(r);

        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::SpaceAround)
            .areas(r);

        let [popup_layout] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::SpaceAround)
            .areas(area);
        popup_layout
    }
    fn extended_area(&self, r: Rect) -> Rect {
        let [area] = Layout::vertical([Constraint::Percentage(80)])
            .flex(Flex::Center)
            .areas(r);

        let [popup_layout] = Layout::horizontal([Constraint::Percentage(80)])
            .flex(Flex::Center)
            .areas(area);
        popup_layout
    }
    fn default_width(&self, r: Rect) -> u16 {
        r.width / 2
    }
    fn default_height(&self, r: Rect) -> u16 {
        match r.height {
            0..=40 => r.height,
            _ => r.height / 2,
        }
    }
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

#[derive(Default)]
pub struct TextAreaWrapper {
    inner: TextArea<'static>,
    title: Option<String>,
    borders: bool,
}

impl std::fmt::Debug for TextAreaWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.lines().join("").as_str())
    }
}

impl Clone for TextAreaWrapper {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            title: self.title.clone(),
            borders: self.borders,
        }
    }
}

impl From<(Option<String>, Option<String>, bool)> for TextAreaWrapper {
    fn from(value: (Option<String>, Option<String>, bool)) -> Self {
        TextAreaWrapper {
            inner: TextArea::from(value.1),
            title: value.0,
            borders: value.2,
        }
    }
}

impl TextAreaWrapper {
    pub fn new(
        title: Option<String>,
        content: Option<String>,
        theme: Theme,
        borders: bool,
    ) -> Self {
        let mut text_area = Self::from((title.clone(), content, borders));
        text_area.with_style(theme);
        text_area
    }

    pub fn borderless(theme: Theme) -> Self {
        let mut text_area = Self::default();
        text_area.with_style(theme);
        text_area
    }

    pub fn bordered(theme: Theme) -> Self {
        let mut text_area = Self::from((Some("".to_string()), None, true));
        text_area.with_style(theme);
        text_area
    }

    fn highlight(&mut self, block_style: Style, cursor_style: Style) {
        self.inner.set_tab_length(2);
        self.inner.set_style(
            self.inner
                .style()
                .remove_modifier(ratatui::style::Modifier::DIM),
        );
        self.inner.set_cursor_style(cursor_style);
        self.with_block_style(block_style);
    }

    fn with_style(&mut self, theme: Theme) -> &mut Self {
        self.inner.set_tab_length(2);
        self.inner
            .set_style(theme.base().add_modifier(Modifier::DIM));
        self.inner.set_cursor_line_style(theme.base());
        self.inner.set_cursor_style(theme.base());
        self.with_block_style(theme.base());
        self
    }

    fn with_block_style(&mut self, style: Style) {
        let block = self.inner.block();
        let borders = match self.borders {
            true => Borders::ALL,
            false => Borders::NONE,
        };

        match block {
            Some(b) => self.inner.set_block(b.clone().style(style)),
            _ => {
                let b = match &self.title {
                    Some(t) => Block::default()
                        .title(t.clone())
                        .borders(borders)
                        .style(style),
                    _ => Block::default().borders(borders).style(style),
                };
                self.inner.set_block(b);
            }
        }
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

fn color_hits<E>(result: &'_ FuzzyResult<E>, style: Style) -> ListItem<'_>
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
            text.push(Span::styled(char.to_string(), style.fg(Color::Yellow)));
            hits_index += 1;
        } else {
            text.push(Span::styled(char.to_string(), style));
        }
    }
    ListItem::new(Line::from(text))
}
