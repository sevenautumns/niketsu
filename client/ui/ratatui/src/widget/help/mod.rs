use once_cell::sync::Lazy;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Cell, Padding, Row, StatefulWidget, Table, Widget};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

use crate::handler::State;

use super::OverlayWidgetState;

struct HelpTab {
    description: String,
    items: Vec<Vec<String>>,
}

static GENERAL: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "General instructions for moving between the widgets".to_string(),
    items: vec![
        vec!["Move left".to_string(), "← <Arrow Left>".to_string()],
        vec!["Move right".to_string(), "→ <Arrow Right>".to_string()],
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Enter options".to_string(), "␣ <Space>".to_string()],
        vec![
            "Enter widget selection".to_string(),
            "⏎ <Enter>".to_string(),
        ],
        vec!["Exit widget selection".to_string(), "⎋ <Esc>".to_string()],
        vec!["Exit application".to_string(), "↵ <q>".to_string()],
    ],
});

static ROOM: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Room is a scrollable list of users".to_string(),
    items: vec![
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
    ],
});

static DATABASE: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Database shows numbers of files available from all media paths".to_string(),
    items: vec![
        vec!["Reload files".to_string(), "␣ + s <Space + s>".to_string()],
        vec!["Stop loading".to_string(), "␣ + p <Space + p>".to_string()],
    ],
});

static CHAT: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Chat consists of scrollable list of messages and chat input".to_string(),
    items: vec![
        vec!["Goto first message".to_string(), "⇱ <Home>".to_string()],
        vec!["Goto last message".to_string(), "⇲ <End>".to_string()],
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Move up 5 messages".to_string(), "⇞ <Page Up>".to_string()],
        vec![
            "Move down 5 messages".to_string(),
            "⇟ <Page Down>".to_string(),
        ],
        vec!["Send message (input)".to_string(), "⏎ <Enter>".to_string()],
    ],
});

static RECENTLY: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Shows recently added videos (monthly, weekly or daily)".to_string(),
    items: vec![
        vec!["Goto first file".to_string(), "⇱ <Home>".to_string()],
        vec!["Goto last file".to_string(), "⇲ <End>".to_string()],
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Move up 5 files".to_string(), "⇞ <Page Up>".to_string()],
        vec!["Move down 5 files".to_string(), "⇟ <Page Down>".to_string()],
        vec!["Move to next timespan".to_string(), "↹ <Tab>".to_string()],
        vec!["Move selection down".to_string(), "<x>".to_string()],
        vec![
            "Push selection into playlist".to_string(),
            "⏎ <Enter>".to_string(),
        ],
    ],
});

static PLAYLIST: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Playlist shows list of files and current file selection".to_string(),
    items: vec![
        vec!["Goto first file".to_string(), "⇱ <Home>".to_string()],
        vec!["Goto last file".to_string(), "⇲ <End>".to_string()],
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Move up 5 files".to_string(), "⇞ <Page Up>".to_string()],
        vec!["Move down 5 files".to_string(), "⇟ <Page Down>".to_string()],
        vec!["Select video".to_string(), "⏎ <Enter>".to_string()],
        vec!["Goto current file".to_string(), "⌫ <Backspace>".to_string()],
        vec!["Move selection down".to_string(), "<x>".to_string()],
        vec![
            "Delete selection (clipboard)".to_string(),
            "<d>".to_string(),
        ],
        vec!["Yank selection (clipboard)".to_string(), "<y>".to_string()],
        vec!["Paste selection (clipboard)".to_string(), "<p>".to_string()],
        vec!["Reverse selection".to_string(), "<r>".to_string()],
        vec!["Highlight current file".to_string(), "<f>".to_string()],
        vec![
            "Paste clipboard".to_string(),
            "ˆ + v <Control + v>".to_string(),
        ],
    ],
});

static MEDIA: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Media paths to directory for scanning (database)".to_string(),
    items: vec![
        vec!["Goto first file".to_string(), "⇱ <Home>".to_string()],
        vec!["Goto last file".to_string(), "⇲ <End>".to_string()],
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Move up 5 files".to_string(), "⇞ <Page Up>".to_string()],
        vec!["Move down 5 files".to_string(), "⇟ <Page Down>".to_string()],
        vec!["Select video".to_string(), "⏎ <Enter>".to_string()],
    ],
});

static SEARCH: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Fuzzy search for all files in the database".to_string(),
    items: vec![
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Move selection down".to_string(), "<x>".to_string()],
        vec![
            "Push selection into playlist".to_string(),
            "⏎ <Enter>".to_string(),
        ],
    ],
});

static LOGIN: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Login to a room".to_string(),
    items: vec![
        vec!["Move up field".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down field".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Try connecting".to_string(), "⏎ <Enter>".to_string()],
    ],
});

static PLAYLISTBROWSER: Lazy<HelpTab> = Lazy::new(|| HelpTab {
    description: "Browser for recent playlists".to_string(),
    items: vec![
        vec!["Move up".to_string(), "↑ <Arrow Up>".to_string()],
        vec!["Move down".to_string(), "↓ <Arrow Down>".to_string()],
        vec!["Select playlist".to_string(), "⏎ <Enter>".to_string()],
    ],
});

pub struct HelpWidget;

#[derive(Debug, Default, Clone)]
pub struct HelpWidgetState {
    current_tab: HelpWidgetTab,
}

#[derive(Debug, Default, PartialEq, Copy, Clone, EnumString, EnumIter, Display)]
pub enum HelpWidgetTab {
    #[default]
    General,
    Room,
    Database,
    Chat,
    Recently,
    Playlist,
    Media,
    Search,
    Login,
    PlaylistBrowser,
}

impl HelpWidgetTab {
    fn next(self) -> Self {
        let variants: Vec<_> = HelpWidgetTab::iter().collect();
        let pos = variants.iter().position(|&v| v == self).unwrap();
        variants[(pos + 1) % variants.len()]
    }

    fn previous(self) -> Self {
        let variants: Vec<_> = HelpWidgetTab::iter().collect();
        let pos = variants.iter().position(|&v| v == self).unwrap();
        variants[(pos + variants.len() - 1) % variants.len()]
    }
}

impl HelpWidgetState {
    pub fn next(&mut self) {
        self.current_tab = self.current_tab.next()
    }

    pub fn previous(&mut self) {
        self.current_tab = self.current_tab.previous()
    }

    pub fn select(&mut self, state: &State) {
        match state {
            State::Chat(_) => self.current_tab = HelpWidgetTab::Chat,
            State::ChatInput(_) => self.current_tab = HelpWidgetTab::Chat,
            State::Users(_) => self.current_tab = HelpWidgetTab::Room,
            State::Playlist(_) => self.current_tab = HelpWidgetTab::Playlist,
            State::Recently(_) => self.current_tab = HelpWidgetTab::Recently,
        }
    }

    pub fn reset(&mut self) {
        self.current_tab = HelpWidgetTab::default();
    }
}

impl OverlayWidgetState for HelpWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(20),
                    Constraint::Min(30),
                    Constraint::Percentage(20),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(20),
                    Constraint::Min(70),
                    Constraint::Percentage(20),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for HelpWidget {
    type State = HelpWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let help_block = Block::default()
            .title("Help")
            .borders(Borders::ALL)
            .title_bottom("← →: navigate tabs");

        let help_page = match state.current_tab {
            HelpWidgetTab::General => &GENERAL,
            HelpWidgetTab::Room => &ROOM,
            HelpWidgetTab::Database => &DATABASE,
            HelpWidgetTab::Chat => &CHAT,
            HelpWidgetTab::Recently => &RECENTLY,
            HelpWidgetTab::Playlist => &PLAYLIST,
            HelpWidgetTab::Media => &MEDIA,
            HelpWidgetTab::Search => &SEARCH,
            HelpWidgetTab::Login => &LOGIN,
            HelpWidgetTab::PlaylistBrowser => &PLAYLISTBROWSER,
        };

        let description = &help_page.description;
        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .horizontal_margin(1)
            .vertical_margin(1)
            .split(area);

        let width_col1 = 35;
        let width_col2 = 25;
        let max_width = width_col1 + width_col2;

        let table_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(max_width), Constraint::Min(0)])
            .split(layout[1]);

        let rows = help_page.items.iter().map(|line| {
            let items = line.iter().map(|r| Cell::from(r.clone()));
            Row::new(items)
        });

        let widths = [
            Constraint::Length(width_col1),
            Constraint::Length(width_col2),
        ];
        let header = Row::new(vec!["Description", "Control"])
            .style(Style::default().fg(Color::Gray).bg(Color::DarkGray));
        let table = Table::new(rows, widths).header(header).block(
            Block::default()
                .title(state.current_tab.to_string())
                .borders(Borders::TOP)
                .padding(Padding::new(1, 0, 0, 1)),
        );

        let empty_space = Block::default().borders(Borders::TOP);

        help_block.render(area, buf);
        description.render(layout[0], buf);
        Widget::render(table, table_layout[0], buf);
        empty_space.render(table_layout[1], buf);
    }
}
