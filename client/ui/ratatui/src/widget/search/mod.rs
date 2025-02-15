use delegate::delegate;
use niketsu_core::file_database::fuzzy::FuzzySearch;
use niketsu_core::file_database::{FileEntry, FileStore};
use niketsu_core::util::FuzzyResult;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, List, ListItem, Padding, StatefulWidget, Widget};
use tui_textarea::Input;

use super::nav::ListNavigationState;
use super::{OverlayWidgetState, TextAreaWrapper};

pub struct SearchWidget;

#[derive(Debug, Default, Clone)]
pub struct SearchWidgetState {
    file_database: FileStore,
    num_files: Option<usize>,
    current_result: Option<Vec<FuzzyResult<FileEntry>>>,
    input_field: TextAreaWrapper,
    nav_state: ListNavigationState,
    style: Style,
}

impl SearchWidgetState {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget.select(Some(0));
        widget
    }

    fn setup_input_field(&mut self) {
        self.input_field
            .with_block(
                Block::default()
                    .borders(Borders::NONE)
                    .padding(Padding::new(1, 0, 0, 0)),
            )
            .with_placeholder("Enter your search")
            .highlight(Style::default(), self.style.dark_gray().on_white());
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn get_selected(&self) -> Option<Vec<FileEntry>> {
        match self.nav_state.selection_range() {
            Some(range) => self.current_result.as_ref().map(|result| {
                result
                    .iter()
                    .skip(range.lower)
                    .take(range.len().saturating_add(1))
                    .map(|r| r.entry.clone())
                    .collect()
            }),
            None => None,
        }
    }

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
        self.num_files = Some(self.file_database.len());
        self.nav_state
            .set_list_len(self.num_files.unwrap_or_default());
    }

    pub fn set_result(&mut self, results: Vec<FuzzyResult<FileEntry>>) {
        if results.is_empty() {
            self.select(None);
        } else if self.selected().is_none() {
            self.select(Some(0));
        }
        self.nav_state.limit(self.len());
        self.current_result = Some(results);
    }

    pub fn reset_all(&mut self) {
        self.current_result = None;
        self.select(Some(0));
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
        self.nav_state.reset_offset();
    }

    fn len(&self) -> usize {
        match self.current_result.clone() {
            Some(vec) => vec.len(),
            None => 0,
        }
    }

    pub fn fuzzy_search(&self, query: String) -> FuzzySearch {
        self.file_database.fuzzy_search(query)
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
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
            pub fn increase_selection_offset(&mut self);
        }
    }
}

impl OverlayWidgetState for SearchWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for SearchWidget {
    type State = SearchWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default()
            .title("Search")
            .borders(Borders::ALL)
            .gray();

        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .horizontal_margin(1)
            .vertical_margin(1)
            .split(area);

        let search_result: Vec<ListItem> = match &state.current_result {
            Some(result) => match state.nav_state.selection_range() {
                Some(range) => result
                    .iter()
                    .take(range.lower)
                    .map(|r| color_hits(r, None))
                    .chain(
                        result
                            .iter()
                            .skip(range.lower)
                            .take(range.len().saturating_add(1))
                            .map(|r| color_hits(r, Some(Color::Cyan))),
                    )
                    .chain(
                        result
                            .iter()
                            .skip(range.upper.saturating_add(1))
                            .map(|r| color_hits(r, None)),
                    )
                    .collect(),
                None => Vec::default(),
            },
            None => Vec::default(),
        };

        let filtered_files = search_result.len();
        let num_files = state.num_files.unwrap_or_default();
        let search_list = List::new(search_result)
            .gray()
            .block(
                Block::default()
                    .style(state.style)
                    .title("Results")
                    .title_top(
                        Line::from(format!("{}/{}", filtered_files, num_files)).right_aligned(),
                    )
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 0, 0, 1)),
            )
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        outer_block.render(area, buf);
        state.input_field.render(layout[0], buf);
        StatefulWidget::render(search_list, layout[1], buf, state.nav_state.inner());
    }
}

fn color_hits(result: &FuzzyResult<FileEntry>, color: Option<Color>) -> ListItem {
    let mut text = Vec::new();
    let name = result.entry.file_name();
    let hits = &result.hits;
    let mut hits_index = 0;
    let hits_len = hits.len();
    for (index, char) in name.char_indices() {
        if hits_index < hits_len && index == hits[hits_index] {
            text.push(Span::styled(
                char.to_string(),
                Style::default().fg(color.unwrap_or(Color::Yellow)),
            ));
            hits_index += 1;
        } else {
            text.push(Span::styled(
                char.to_string(),
                Style::default().fg(color.unwrap_or(Color::Gray)),
            ));
        }
    }
    ListItem::new(Line::from(text))
}
