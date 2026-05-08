use delegate::delegate;
use niketsu_core::playlist::Playlist;
use niketsu_core::playlist::file::{NamedPlaylist, PlaylistBrowser};
use niketsu_core::util::FuzzyResult;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Padding, StatefulWidget, Widget};
use tui_textarea::Input;

use super::nav::ListNavigationState;
use super::{OverlayWidgetState, TextAreaWrapper};
use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

pub struct PlaylistBrowserWidget;

#[derive(Debug)]
pub struct PlaylistBrowserWidgetState {
    playlist_browser: PlaylistBrowser,
    num_files: Option<usize>,
    fuzzy_result: Vec<FuzzyResult<NamedPlaylist>>,
    input_field: TextAreaWrapper,
    nav_state: ListNavigationState,
    theme: ThemeWrapper,
}

impl ThemedWidget for PlaylistBrowserWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl PlaylistBrowserWidgetState {
    pub fn new(theme: Theme) -> Self {
        let mut widget = Self {
            theme: ThemeWrapper::new(theme),
            input_field: TextAreaWrapper::borderless(theme),
            num_files: None,
            fuzzy_result: Default::default(),
            nav_state: Default::default(),
            playlist_browser: Default::default(),
        };
        widget.select(Some(0));
        widget
    }

    pub fn set_playlist_browser(&mut self, playlist_browser: PlaylistBrowser) {
        self.playlist_browser = playlist_browser;
        self.num_files = Some(
            self.playlist_browser
                .playlist_map()
                .values()
                .map(|p| p.len())
                .sum(),
        );
        self.fuzzy_result = self.playlist_browser.fuzzy_search("");
        self.nav_state
            .set_list_len(self.num_files.unwrap_or_default());
        self.select(Some(0));
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn get_playlist(&self) -> Option<Playlist> {
        if let Some(pos) = self.selected()
            && let Some(playlist) = self.fuzzy_result.get(pos)
        {
            return Some(playlist.entry.playlist.get_playlist());
        }
        None
    }

    pub fn reset_all(&mut self) {
        self.select(Some(0));
        self.input_field = TextAreaWrapper::borderless(self.theme.inner());
        self.fuzzy_result = self.playlist_browser.fuzzy_search("");
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
        let query = self.input_field.get_input();
        self.fuzzy_result = self.playlist_browser.fuzzy_search(&query);
        if self.selected().is_none() & !self.fuzzy_result.is_empty() {
            self.select(Some(0));
        }
    }

    delegate! {
        to self.nav_state {
            pub fn next(&mut self);
            pub fn previous(&mut self);
            pub fn jump_next(&mut self, offset: usize);
            pub fn jump_previous(&mut self, offset: usize);
            pub fn jump_start(&mut self);
            pub fn jump_end(&mut self);
            pub fn selected(&self) -> Option<usize>;
            pub fn select(&mut self, index: Option<usize>);
        }
    }
}

impl OverlayWidgetState for PlaylistBrowserWidgetState {
    fn area(&self, r: Rect) -> Rect {
        self.extended_area(r)
    }
}

impl StatefulWidget for PlaylistBrowserWidget {
    type State = PlaylistBrowserWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let style = state.theme.style();
        let highlight_style = state.theme.highlight();

        let outer_block = Block::default()
            .title("Playlists")
            .borders(Borders::ALL)
            .style(style);

        let horizontal_blocks =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(area);

        let left_layout = Layout::default()
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(3),
                ]
                .as_ref(),
            )
            .horizontal_margin(1)
            .split(horizontal_blocks[0]);

        let right_layout = Layout::default()
            .constraints([Constraint::Min(3)].as_ref())
            .horizontal_margin(1)
            .vertical_margin(1)
            .split(horizontal_blocks[1]);

        //TODO mark hits
        let playlists: Vec<ListItem> = state
            .fuzzy_result
            .iter()
            .map(|playlist| {
                ListItem::new(
                    Line::from(format!("{}/{}", playlist.entry.room, playlist.entry.name))
                        .style(style),
                )
            })
            .collect();

        let filtered_files = state.fuzzy_result.len();
        let num_files = state.num_files.unwrap_or_default();
        let playlists_block = List::new(playlists)
            .block(
                Block::default()
                    .style(style)
                    .title("Results")
                    .title_top(Line::from(format!("{filtered_files}/{num_files}")).right_aligned())
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 0, 0, 1)),
            )
            .highlight_style(highlight_style)
            .highlight_symbol("> ");

        let mut playlist_content = Vec::<ListItem>::new();
        if let Some(pos) = state.selected() {
            let current_video = state.fuzzy_result.get(pos);
            if let Some(v) = current_video {
                playlist_content = v
                    .entry
                    .playlist
                    .get_playlist()
                    .iter()
                    .map(|video| ListItem::new(Line::from(video.as_str().to_string())))
                    .collect();
            }
        }

        let playlist_content_block = List::new(playlist_content)
            .gray()
            .block(
                Block::default()
                    .style(style)
                    .borders(Borders::LEFT)
                    .padding(Padding::new(1, 0, 0, 1)),
            )
            .highlight_style(highlight_style)
            .highlight_symbol("> ");

        let input_field = state
            .input_field
            .with_style(state.theme.inner())
            .with_placeholder("Enter a path separated by /");
        input_field.highlight(state.theme.base(), highlight_style);

        outer_block.render(area, buf);
        input_field.render(left_layout[1], buf);
        StatefulWidget::render(
            playlists_block,
            left_layout[2],
            buf,
            state.nav_state.inner(),
        );
        Widget::render(playlist_content_block, right_layout[0], buf);
    }
}
