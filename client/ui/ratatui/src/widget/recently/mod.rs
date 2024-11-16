use std::cmp::Reverse;
use std::time::{Duration, SystemTime};

use niketsu_core::file_database::{FileEntry, FileStore};
use ratatui::prelude::{Buffer, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{Borders, List, ListItem, StatefulWidget};
use strum::{Display, EnumCount, EnumIter, FromRepr};

use super::ListStateWrapper;

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
}

#[derive(Debug, Default, Clone)]
pub struct RecentlyWidget;

#[derive(Debug, Default, Clone)]
pub struct RecentlyWidgetState {
    frequency: Frequency,
    file_database: FileStore,
    recent_videos: Vec<FileEntry>,
    num_files: Option<usize>,
    list_state: ListStateWrapper,
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

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
        self.num_files = Some(self.file_database.len());
        self.recent_videos = self.filter_file_database();
        if self.len() > 0 && self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
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

    fn get(&self, index: usize) -> Option<&FileEntry> {
        self.recent_videos.get(index)
    }

    pub fn next(&mut self) {
        self.list_state.next();
    }

    pub fn previous(&mut self) {
        self.list_state.limited_previous(self.len());
    }

    pub fn get_selected(&self) -> Option<&FileEntry> {
        match self.list_state.selected() {
            Some(i) => self.get(i),
            None => None,
        }
    }

    fn len(&self) -> usize {
        self.recent_videos.len()
    }
}

impl StatefulWidget for RecentlyWidget {
    type State = RecentlyWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let recently_added: Vec<ListItem> = state
            .recent_videos
            .iter()
            .map(|v| ListItem::from(v.file_name()))
            .collect();

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

        StatefulWidget::render(video_list, area, buf, state.list_state.inner());
    }
}
