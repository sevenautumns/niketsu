use niketsu_core::playlist::{Playlist, Video};
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::Title;
use ratatui::widgets::{
    Block, Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;

//TODO negative offset support
//TODO wrap length
//TODO fix list state
pub struct PlaylistWidget;

#[derive(Debug, Default, Clone)]
pub struct PlaylistWidgetState {
    playlist: Playlist,
    playing_video: Option<Video>,
    list_state: ListStateWrapper,
    vertical_scroll_state: ScrollbarState,
    selection_offset: usize,
    clipboard: Option<Vec<Video>>,
    style: Style,
}

impl PlaylistWidgetState {
    pub fn set_playlist(&mut self, playlist: Playlist) {
        self.playlist = playlist;
    }

    pub fn set_playing_video(&mut self, playing_video: Option<Video>) {
        self.playing_video = playing_video;
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn next(&mut self) {
        self.selection_offset = 0;
        self.list_state.overflowing_next(self.playlist.len());
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i as u16);
        }
    }

    pub fn previous(&mut self) {
        self.selection_offset = 0;
        self.list_state.overflowing_previous(self.playlist.len());
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i as u16);
        }
    }

    pub fn jump_next(&mut self, offset: usize) {
        self.list_state.jump_next(offset)
    }

    pub fn jump_previous(&mut self, offset: usize) {
        self.list_state
            .limited_jump_previous(offset, self.playlist.len());
    }

    pub fn reset_offset(&mut self) {
        self.selection_offset = 0;
    }

    pub fn increase_selection_offset(&mut self) {
        if let Some(i) = self.list_state.selected() {
            if self.selection_offset.saturating_add(i) < self.playlist.len().saturating_sub(1) {
                self.selection_offset += 1;
            }
        };
    }

    pub fn get_current_video(&self) -> Option<&Video> {
        match self.list_state.selected() {
            Some(index) => self.playlist.get(index),
            None => None,
        }
    }

    pub fn get_current_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn yank_clipboard(&mut self) -> Option<(usize, usize)> {
        match self.list_state.selected() {
            Some(index) => {
                self.clipboard = Some(
                    self.playlist
                        .get_range(index, index + self.selection_offset)
                        .cloned()
                        .collect(),
                );
                Some((index, index + self.selection_offset))
            }
            None => None,
        }
    }

    pub fn get_clipboard(&self) -> Option<Vec<Video>> {
        self.clipboard.clone()
    }
}

impl StatefulWidget for PlaylistWidget {
    type State = PlaylistWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let scroll_block = Block::default()
            .title(Title::from("Playlist"))
            .borders(Borders::ALL)
            .style(state.style);

        //TODO refactor
        let playlist = state.playlist.clone();
        let playlist: Vec<ListItem> = match state.list_state.selected() {
            Some(index) => playlist
                .iter()
                .take(index)
                .map(|t| {
                    if let Some(video) = state.playing_video.clone() {
                        let video_text = format!("> {}", t.as_str());
                        if t.eq(&video) {
                            return ListItem::new(vec![Line::styled(
                                video_text,
                                Style::default().fg(Color::Yellow),
                            )]);
                        }
                    }
                    ListItem::new(vec![Line::from(t.as_str().gray())])
                })
                .chain(
                    playlist
                        .iter()
                        .skip(index)
                        .take(state.selection_offset + 1)
                        .map(|t| {
                            if let Some(video) = state.playing_video.clone() {
                                let video_text = format!("> {}", t.as_str());
                                if t.eq(&video) {
                                    return ListItem::new(vec![Line::styled(
                                        video_text,
                                        Style::default().fg(Color::Yellow),
                                    )]);
                                }
                            }
                            ListItem::new(vec![Line::from(t.as_str().cyan())])
                        }),
                )
                .chain(
                    playlist
                        .iter()
                        .skip(index + state.selection_offset + 1)
                        .map(|t| {
                            if let Some(video) = state.playing_video.clone() {
                                let video_text = format!("> {}", t.as_str());
                                if t.eq(&video) {
                                    return ListItem::new(vec![Line::styled(
                                        video_text,
                                        Style::default().fg(Color::Yellow),
                                    )]);
                                }
                            }
                            ListItem::new(vec![Line::from(t.as_str().gray())])
                        }),
                )
                .collect(),
            None => playlist
                .iter()
                .map(|t| {
                    if let Some(video) = state.playing_video.clone() {
                        let video_text = format!("> {}", t.as_str());
                        if t.eq(&video) {
                            return ListItem::new(vec![Line::styled(
                                video_text,
                                Style::default().fg(Color::Yellow),
                            )]);
                        }
                    }
                    ListItem::new(vec![Line::from(t.as_str().gray())])
                })
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

        StatefulWidget::render(list, area, buf, state.list_state.inner());

        let mut state = state.vertical_scroll_state;
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
}
