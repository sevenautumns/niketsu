use niketsu_core::playlist::{Playlist, Video};
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{
    Borders, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
};

use super::ListStateWrapper;
pub(crate) mod video_overlay;

//TODO negative offset support
//TODO state trait wrapper
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
        if !self.playlist.is_empty() && self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
    }

    pub fn set_playing_video(&mut self, playing_video: Option<Video>) {
        self.playing_video = playing_video;
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    fn set_vertical_scroll_state(&mut self) {
        if let Some(i) = self.list_state.selected() {
            self.vertical_scroll_state = self.vertical_scroll_state.position(i);
        }
    }

    pub fn next(&mut self) {
        self.selection_offset = 0;
        self.list_state.overflowing_next(self.playlist.len());
        self.set_vertical_scroll_state();
    }

    pub fn previous(&mut self) {
        self.selection_offset = 0;
        self.list_state.overflowing_previous(self.playlist.len());
        self.set_vertical_scroll_state();
    }

    pub fn jump_next(&mut self, offset: usize) {
        self.list_state.jump_next(offset);
        self.set_vertical_scroll_state();
    }

    pub fn jump_previous(&mut self, offset: usize) {
        self.list_state
            .limited_jump_previous(offset, self.playlist.len());
        self.set_vertical_scroll_state();
    }

    pub fn jump_start(&mut self) {
        self.list_state.select(Some(0));
        self.set_vertical_scroll_state();
    }

    pub fn jump_end(&mut self) {
        self.list_state
            .select(Some(self.playlist.len().saturating_sub(1)));
        self.set_vertical_scroll_state();
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

        let playlist: Vec<ListItem> = match state.list_state.selected() {
            Some(index) => state
                .playlist
                .iter()
                .take(index)
                .map(|t| color_selection(t, state, Color::Gray, Color::Yellow))
                .chain(
                    state
                        .playlist
                        .iter()
                        .skip(index)
                        .take(state.selection_offset + 1)
                        .map(|t| color_selection(t, state, Color::Cyan, Color::Cyan)),
                )
                .chain(
                    state
                        .playlist
                        .iter()
                        .skip(index + state.selection_offset + 1)
                        .map(|t| color_selection(t, state, Color::Gray, Color::Yellow)),
                )
                .collect(),
            None => state
                .playlist
                .iter()
                .map(|t| color_selection(t, state, Color::Gray, Color::Yellow))
                .collect(),
        };

        let playlist_len = playlist.len();
        let list = List::new(playlist)
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
        state = state.content_length(playlist_len);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }
}

fn color_selection<'a>(
    video: &'a Video,
    state: &PlaylistWidgetState,
    default_color: Color,
    hightlight_color: Color,
) -> ListItem<'a> {
    if let Some(playing_video) = &state.playing_video {
        if video.eq(playing_video) {
            let video_text = format!("> {}", video.as_str());
            return ListItem::new(vec![Line::styled(
                video_text,
                Style::default().fg(hightlight_color),
            )]);
        }
    }
    ListItem::new(vec![Line::from(video.as_str().fg(default_color))])
}
