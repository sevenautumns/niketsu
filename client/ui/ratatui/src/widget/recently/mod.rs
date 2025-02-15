use std::cmp::Reverse;
use std::time::{Duration, SystemTime};

use delegate::delegate;
use niketsu_core::file_database::{FileEntry, FileStore};
use ratatui::prelude::{Buffer, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{Borders, List, ListItem, StatefulWidget};
use strum::{Display, EnumCount, EnumIter, FromRepr};

use super::nav::ListNavigationState;

#[derive(Debug, Default, Clone, Copy, Display, FromRepr, EnumIter, EnumCount)]
enum Frequency {
    #[default]
    #[strum(to_string = "Daily")]
    Daily,
    #[strum(to_string = "Weekly")]
    Weekly,
    #[strum(to_string = "Monthly")]
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
    style: Style,
}

impl RecentlyWidgetState {
    pub fn new() -> Self {
        Self {
            frequency: Frequency::Monthly,
            ..Default::default()
        }
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
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
        self.nav_state.set_list_len(self.file_database.len());
        if self.len() > 0 && self.selected().is_none() {
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
        let recently_added: Vec<ListItem> = match state.nav_state.selection_range() {
            Some(range) => state
                .recent_videos
                .iter()
                .take(range.lower)
                .map(|v| ListItem::from(v.file_name()))
                .chain(
                    state
                        .recent_videos
                        .iter()
                        .skip(range.lower)
                        .take(range.len().saturating_add(1))
                        .map(|v| {
                            ListItem::from(vec![Line::style(
                                v.file_name().into(),
                                Style::default().fg(Color::Cyan),
                            )])
                        }),
                )
                .chain(
                    state
                        .recent_videos
                        .iter()
                        .skip(range.upper.saturating_add(1))
                        .map(|v| ListItem::from(v.file_name())),
                )
                .collect(),
            None => state
                .recent_videos
                .iter()
                .map(|v| ListItem::from(v.file_name()))
                .collect(),
        };

        let list_block = Block::default()
            .title_bottom(Line::from(format!("({})", state.len())).right_aligned())
            .borders(Borders::ALL)
            .style(state.style)
            .title(Title::from(format!(
                "Recently added videos ({})",
                state.frequency
            )));

        let video_list = List::new(recently_added)
            .gray()
            .block(list_block)
            .highlight_symbol("> ")
            .highlight_style(Style::default().fg(Color::Cyan));

        StatefulWidget::render(video_list, area, buf, state.nav_state.inner());
    }
}
