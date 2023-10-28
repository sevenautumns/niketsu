use niketsu_core::file_database::fuzzy::{FuzzyResult, FuzzySearch};
use niketsu_core::file_database::{FileEntry, FileStore};
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, List, ListItem, ListState, Padding, StatefulWidget, Widget};
use ratatui_textarea::Input;

use super::{ListStateWrapper, OverlayWidgetState, TextAreaWrapper};

//TODO add item where cursor is
#[derive(Debug, Default, Clone)]
pub struct FuzzySearchWidget {
    file_database: FileStore,
    current_result: Option<Vec<FuzzyResult>>,
    input_field: TextAreaWrapper,
    state: ListStateWrapper,
    style: Style,
}

impl FuzzySearchWidget {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget.state.select(Some(0));
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
        self.state.clone_inner()
    }

    pub fn get_selected(&self) -> Option<FileEntry> {
        match self.state.selected() {
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
            self.state.select(None);
        }
        self.current_result = Some(results);
    }

    pub fn reset_all(&mut self) {
        self.current_result = None;
        self.state.select(Some(0));
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
    }

    pub fn next(&mut self) {
        let len = self.len();
        if len == 0 {
            self.state.select(None);
        } else {
            self.state.overflowing_next(len);
        }
    }

    pub fn previous(&mut self) {
        let len = self.len();
        if len == 0 {
            self.state.select(None);
        } else {
            self.state.overflowing_previous(len);
        }
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
}

impl OverlayWidgetState for FuzzySearchWidget {
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
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default()
            .title("Search")
            .borders(Borders::ALL)
            .gray();

        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .margin(1)
            .split(area);

        let search_result = self.current_result.clone();
        let search_result: Vec<ListItem> = match search_result {
            Some(result) => result
                .into_iter()
                .map(|s| {
                    let mut text = Vec::new();
                    let name = s.entry.file_name();
                    let hits = s.hits;
                    let mut hits_index = 0;
                    for (index, char) in name.char_indices() {
                        if index < hits.len() && index == hits[hits_index] {
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
                    .style(self.style)
                    .title("Results")
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 1, 1, 1)),
            )
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        let input_field = self.input_field.clone();
        outer_block.render(area, buf);
        input_field.widget().render(layout[0], buf);
        StatefulWidget::render(search_list, layout[1], buf, state);
    }
}
