use delegate::delegate;
use niketsu_core::playlist::{Playlist, Video};
use ratatui::prelude::{Buffer, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::scrollbar;
use ratatui::text::Line;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{Borders, List, ListItem, Scrollbar, ScrollbarOrientation, StatefulWidget};

use super::nav::ListNavigationState;

pub(crate) mod video_overlay;

pub struct PlaylistWidget;

#[derive(Debug, Default, Clone)]
pub struct PlaylistWidgetState {
    playlist: Playlist,
    playing_video: Option<Video>,
    nav_state: ListNavigationState,
    clipboard: Option<Vec<Video>>,
    video_share: bool,
    style: Style,
}

impl PlaylistWidgetState {
    pub fn set_playlist(&mut self, playlist: Playlist) {
        self.playlist = playlist;
        self.nav_state.set_list_len(self.playlist.len());
        if !self.playlist.is_empty() && self.nav_state.selected().is_none() {
            self.select(Some(0));
        }
    }

    pub fn set_playing_video(&mut self, playing_video: Option<Video>) {
        self.playing_video = playing_video;
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn get_current_video(&self) -> Option<&Video> {
        match self.nav_state.selected() {
            Some(index) => self.playlist.get(index),
            None => None,
        }
    }

    pub fn yank_clipboard(&mut self) -> Option<(usize, usize)> {
        match self.nav_state.selection_range() {
            Some(range) => {
                self.clipboard = Some(
                    self.playlist
                        .get_range(range.lower, range.upper)
                        .cloned()
                        .collect(),
                );
                Some((range.lower, range.upper))
            }
            None => None,
        }
    }

    pub fn get_clipboard(&self) -> Option<Vec<Video>> {
        self.clipboard.clone()
    }

    pub fn toggle_video_share(&mut self) {
        self.video_share = !self.video_share
    }

    delegate! {
        to self.nav_state {
            pub fn next(&mut self);
            pub fn previous(&mut self);
            pub fn jump_next(&mut self, offset: usize);
            pub fn jump_previous(&mut self, offset: usize);
            pub fn jump_start(&mut self);
            pub fn jump_end(&mut self);
            pub fn reset_offset(&mut self);
            pub fn increase_selection_offset(&mut self);
            pub fn selected(&self) -> Option<usize>;
            pub fn select(&mut self, index: Option<usize>);
        }
    }
}

impl StatefulWidget for PlaylistWidget {
    type State = PlaylistWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let video_share = match state.video_share {
            true => Line::styled("sharing", Style::default().fg(Color::Green)),
            false => Line::styled("not sharing", Style::default().fg(Color::Red)),
        };

        let scroll_block = Block::default()
            .title_top(video_share.right_aligned())
            .title(Title::from("Playlist"))
            .title_bottom(Line::from(format!("({})", state.playlist.len())).right_aligned())
            .borders(Borders::ALL)
            .style(state.style);

        let playlist: Vec<ListItem> = match state.nav_state.selection_range() {
            Some(range) => state
                .playlist
                .iter()
                .take(range.lower)
                .map(|t| color_selection(t, state, Color::Gray, Color::Yellow))
                .chain(
                    state
                        .playlist
                        .iter()
                        .skip(range.lower)
                        .take(range.len().saturating_add(1))
                        .map(|t| color_selection(t, state, Color::Cyan, Color::Cyan)),
                )
                .chain(
                    state
                        .playlist
                        .iter()
                        .skip(range.upper.saturating_add(1))
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

        StatefulWidget::render(list, area, buf, state.nav_state.inner());

        let mut state = state.nav_state.vertical_scroll_state();
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
