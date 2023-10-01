use std::ops::Range;

use niketsu_core::playlist::{Playlist, PlaylistVideo};
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::Title;
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState,
    StatefulWidget,
};

pub(crate) mod handle;

//TODO
#[derive(Debug, Default, Clone)]
pub struct PlaylistWidget {
    state: PlaylistWidgetState,
}

#[derive(Debug, Default, Clone)]
struct PlaylistWidgetState {
    vertical_scroll_state: ScrollbarState,
    playlist: Playlist,
    state: ListState,
    selection_offset: usize,
    clipboard: Option<Vec<PlaylistVideo>>,
    clipboard_range: Option<Range<usize>>,
    style: Style,
}

impl PlaylistWidget {
    pub fn new() -> Self {
        PlaylistWidget {
            state: Default::default(),
        }
    }

    pub fn set_playlist(&mut self, playlist: Playlist) {
        self.state.playlist = playlist;
    }

    pub fn set_style(&mut self, style: Style) {
        self.state.style = style;
    }

    pub fn state(&self) -> ListState {
        self.state.state.clone()
    }

    pub fn next(&mut self) {
        self.state.selection_offset = 0;
        let i = match self.state.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.state.playlist.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        self.state.selection_offset = 0;
        let i = match self.state.state.selected() {
            Some(i) => {
                if i >= self.state.playlist.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    pub fn unselect(&mut self) {
        self.state.state.select(None);
        self.reset_offset();
    }

    pub fn reset_offset(&mut self) {
        self.state.selection_offset = 0;
    }

    //TODO negative offset
    pub fn increase_selection_offset(&mut self) {
        if let Some(i) = self.state.state.selected() {
            if self.state.selection_offset.saturating_add(i)
                < self.state.playlist.len().saturating_sub(1)
            {
                self.state.selection_offset += 1;
            }
        };
    }

    pub fn get_current_video(&self) -> Option<&PlaylistVideo> {
        match self.state.state.selected() {
            Some(index) => self.state.playlist.get(index),
            None => None,
        }
    }

    pub fn get_current_index(&self) -> Option<usize> {
        self.state.state.selected()
    }

    pub fn yank_clipboard(&mut self) -> Option<(usize, usize)> {
        match self.state.state.selected() {
            Some(index) => {
                self.state.clipboard = Some(
                    self.state
                        .playlist
                        .get_range(index, index + self.state.selection_offset)
                        .cloned()
                        .collect(),
                );
                self.state.clipboard_range = None;
                Some((index, index + self.state.selection_offset))
            }
            None => None,
        }
    }

    pub fn yank_clipboard_range(&mut self) {
        match self.state.state.selected() {
            Some(index) => {
                self.state.clipboard_range = Some(index..(index + self.state.selection_offset + 1));
                self.state.clipboard = None;
            }
            None => self.state.clipboard_range = None,
        }
    }

    pub fn get_clipboard(&self) -> Option<Vec<PlaylistVideo>> {
        self.state.clipboard.clone()
    }

    pub fn get_clipboard_range(&self) -> Option<Range<usize>> {
        self.state.clipboard_range.clone()
    }
}

impl StatefulWidget for PlaylistWidget {
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let scroll_block = Block::default()
            .title(Title::from("Playlist"))
            .borders(Borders::ALL)
            .style(self.state.style);

        let playlist = self.state.playlist;
        //TODO calculate boundaries myself...
        let playlist: Vec<ListItem> = match state.selected() {
            Some(index) => playlist
                .iter()
                .take(index)
                .map(|t| ListItem::new(vec![Line::from(t.as_str().gray())]))
                .chain(
                    playlist
                        .iter()
                        .skip(index)
                        .take(self.state.selection_offset + 1)
                        .map(|t| ListItem::new(vec![Line::from(t.as_str().cyan())])),
                )
                .chain(
                    playlist
                        .iter()
                        .skip(index + self.state.selection_offset + 1)
                        .map(|t| ListItem::new(vec![Line::from(t.as_str().gray())])),
                )
                .collect(),
            None => playlist
                .iter()
                .map(|t| ListItem::new(vec![Line::from(t.as_str())]))
                .collect(),
        };

        let list = List::new(playlist.clone())
            .gray()
            .block(scroll_block)
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        StatefulWidget::render(list, area, buf, state);

        let mut state = self.state.vertical_scroll_state;
        state = state.content_length(playlist.len() as u16);
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }

    type State = ListState;
}
