use std::cmp::Reverse;
use std::time::{Duration, SystemTime};

use delegate::delegate;
use niketsu_core::file_database::{FileEntry, FileStore};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, StatefulWidget};
use strum::{Display, EnumCount, EnumIter, FromRepr};

use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

use super::nav::ListNavigationState;

#[derive(Debug, Default, Clone, Copy, Display, FromRepr, EnumIter, EnumCount)]
enum Frequency {
    #[default]
    Daily,
    Weekly,
    Monthly,
}

impl Frequency {
    fn as_duration(&self) -> Duration {
        match self {
            Frequency::Daily => Duration::from_secs(24 * 60 * 60),
            Frequency::Weekly => Duration::from_secs(7 * 24 * 60 * 60),
            Frequency::Monthly => Duration::from_secs(30 * 24 * 60 * 60),
        }
    }

    fn next(&self) -> Frequency {
        match self {
            Frequency::Daily => Frequency::Weekly,
            Frequency::Weekly => Frequency::Monthly,
            Frequency::Monthly => Frequency::Daily,
        }
    }

    fn previous(&self) -> Frequency {
        match self {
            Frequency::Daily => Frequency::Monthly,
            Frequency::Weekly => Frequency::Daily,
            Frequency::Monthly => Frequency::Weekly,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct RecentlyWidget;

#[derive(Debug, Default, Clone)]
pub struct RecentlyWidgetState {
    frequency: Frequency,
    file_database: FileStore,
    recent_videos: Vec<FileEntry>,
    nav_state: ListNavigationState,
    theme: ThemeWrapper,
}

impl ThemedWidget for RecentlyWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl RecentlyWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            frequency: Frequency::Monthly,
            theme: ThemeWrapper::new(theme),
            ..Default::default()
        }
    }

    pub fn next_frequency(&mut self) {
        self.frequency = self.frequency.next();
        self.nav_state.reset_offset();
        self.update_database()
    }

    pub fn previous_frequency(&mut self) {
        self.frequency = self.frequency.previous();
        self.nav_state.reset_offset();
        self.update_database()
    }

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
        self.update_database()
    }

    fn update_database(&mut self) {
        self.recent_videos = self.filter_file_database();
        self.nav_state.set_list_len(self.len());
        if self.len() > 0 && self.selected().is_none_or(|size| size >= self.len()) {
            self.select(Some(0));
        }
    }

    fn filter_file_database(&mut self) -> Vec<FileEntry> {
        let now = SystemTime::now();
        let mut file_entries: Vec<FileEntry> = self
            .file_database
            .into_iter()
            .filter(|f| match f.modified() {
                Some(ts) => match now.duration_since(*ts) {
                    Ok(diff) => diff <= self.frequency.as_duration(),
                    Err(_) => false,
                },
                None => false,
            })
            .cloned()
            .collect();
        file_entries.sort_by_key(|f| Reverse(*f.modified().unwrap()));
        file_entries
    }

    fn len(&self) -> usize {
        self.recent_videos.len()
    }

    pub fn get_selected(&mut self) -> Option<Vec<FileEntry>> {
        match self.nav_state.selection_range() {
            Some(range) => Some(
                self.recent_videos
                    .iter()
                    .skip(range.lower)
                    .take(range.len().saturating_add(1))
                    .cloned()
                    .collect(),
            ),
            None => None,
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
            pub fn reset_offset(&mut self);
            pub fn increase_selection_offset(&mut self);
            pub fn selected(&self) -> Option<usize>;
            pub fn select(&mut self, index: Option<usize>);
        }
    }
}

impl StatefulWidget for RecentlyWidget {
    type State = RecentlyWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let style = state.theme.style();

        let recently_added: Vec<ListItem> = match state.nav_state.selection_range() {
            Some(range) => state
                .recent_videos
                .iter()
                .take(range.lower)
                .map(|v| ListItem::from(v.file_name()).style(style))
                .chain(
                    state
                        .recent_videos
                        .iter()
                        .skip(range.lower)
                        .take(range.len().saturating_add(1))
                        .map(|v| {
                            ListItem::from(vec![Line::style(
                                v.file_name().into(),
                                state.theme.highlight(),
                            )])
                        }),
                )
                .chain(
                    state
                        .recent_videos
                        .iter()
                        .skip(range.upper.saturating_add(1))
                        .map(|v| ListItem::from(v.file_name()).style(style)),
                )
                .collect(),
            None => state
                .recent_videos
                .iter()
                .map(|v| ListItem::from(v.file_name()).style(style))
                .collect(),
        };

        let list_block = Block::default()
            .title_bottom(Line::from(format!("({})", state.len())).right_aligned())
            .borders(Borders::ALL)
            .style(state.theme.style())
            .title(format!("Recently added videos ({})", state.frequency));

        let video_list = List::new(recently_added)
            .gray()
            .block(list_block)
            .highlight_symbol("> ")
            .highlight_style(state.theme.highlight());

        StatefulWidget::render(video_list, area, buf, state.nav_state.inner());
    }
}
