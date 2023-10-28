use niketsu_core::file_database::fuzzy::{FuzzyResult, FuzzySearch};
use niketsu_core::file_database::{FileEntry, FileStore};
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, List, ListItem, ListState, Padding, StatefulWidget, Widget};
use ratatui_textarea::Input;

use super::{ListStateWrapper, OverlayWidgetState, TextAreaWrapper};

pub struct FuzzySearchWidget;

#[derive(Debug, Default, Clone)]
pub struct FuzzySearchWidgetState {
    file_database: FileStore,
    current_result: Option<Vec<FuzzyResult>>,
    input_field: TextAreaWrapper,
    list_state: ListStateWrapper,
    max_len: usize,
    style: Style,
}

impl FuzzySearchWidgetState {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget.list_state.select(Some(0));
        widget.max_len = 200;
        widget
    }

    fn setup_input_field(&mut self) {
        self.input_field
            .set_textarea_style(self.style, self.style.dark_gray().on_white());
        self.input_field
            .set_block(Block::default().padding(Padding::new(1, 0, 0, 0)));
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn get_state(&self) -> ListState {
        self.list_state.clone_inner()
    }

    pub fn get_selected(&self) -> Option<FileEntry> {
        match self.list_state.selected() {
            Some(i) => self
                .current_result
                .clone()
                .map(|result| result[i].clone().entry),
            None => None,
        }
    }

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
    }

    pub fn set_result(&mut self, results: Vec<FuzzyResult>) {
        if results.is_empty() {
            self.list_state.select(None);
        }
        self.current_result = Some(results);
    }

    pub fn reset_all(&mut self) {
        self.current_result = None;
        self.list_state.select(Some(0));
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
    }

    pub fn next(&mut self) {
        let len = self.len();
        if len == 0 {
            self.list_state.select(None);
        } else {
            self.list_state.overflowing_next(len);
        }
    }

    pub fn previous(&mut self) {
        let len = self.len();
        if len == 0 {
            self.list_state.select(None);
        } else {
            self.list_state.overflowing_previous(len);
        }
    }

    fn len(&self) -> usize {
        match self.current_result.clone() {
            Some(vec) => std::cmp::min(self.max_len, vec.len()),
            None => 0,
        }
    }

    pub fn fuzzy_search(&self, query: String) -> FuzzySearch {
        self.file_database.fuzzy_search(query)
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }
}

impl OverlayWidgetState for FuzzySearchWidgetState {
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

impl StatefulWidget for FuzzySearchWidget {
    type State = FuzzySearchWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default()
            .title("Search")
            .borders(Borders::ALL)
            .gray();

        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .margin(1)
            .split(area);

        let search_result: Vec<ListItem> = match &state.current_result {
            Some(result) => result
                .iter()
                .take(state.max_len)
                .map(|s| {
                    let mut text = Vec::new();
                    let name = s.entry.file_name();
                    let hits = &s.hits;
                    let mut hits_index = 0;
                    let hits_len = hits.len();
                    for (index, char) in name.char_indices() {
                        if hits_index < hits_len && index == hits[hits_index] {
                            text.push(Span::styled(
                                char.to_string(),
                                Style::default().fg(Color::Yellow),
                            ));
                            hits_index += 1;
                        } else {
                            text.push(Span::raw(char.to_string()));
                        }
                    }
                    ListItem::new(Line::from(text))
                })
                .collect(),
            None => Vec::default(),
        };

        let search_list = List::new(search_result)
            .gray()
            .block(
                Block::default()
                    .style(state.style)
                    .title("Results")
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 1, 1, 1)),
            )
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        let input_field = state.input_field.clone();
        outer_block.render(area, buf);
        input_field.widget().render(layout[0], buf);
        StatefulWidget::render(search_list, layout[1], buf, state.list_state.inner());
    }
}
