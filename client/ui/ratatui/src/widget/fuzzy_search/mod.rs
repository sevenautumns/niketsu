use niketsu_core::file_database::fuzzy::FuzzyResult;
use niketsu_core::file_database::FileStore;
use ratatui::prelude::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Padding, Widget};

use super::{OverlayWidget, TextAreaWrapper};

pub(crate) mod handle;

#[derive(Debug, Default, Clone)]
pub struct FuzzySearchWidget<'a> {
    file_database: FileStore,
    current_result: Option<Vec<FuzzyResult>>,
    input_field: TextAreaWrapper<'a>,
    state: Option<ListState>,
    style: Style,
}

impl<'a> FuzzySearchWidget<'a> {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget
    }

    fn setup_input_field(&mut self) {
        self.input_field
            .set_textarea_style(self.style, self.style.dark_gray().on_white());
        self.input_field
            .set_block(Block::default().padding(Padding::new(1, 0, 0, 0)));
    }

    pub fn get_input(&self) -> String {
        self.input_field.lines().join("")
    }

    pub fn get_file_database(&self) -> FileStore {
        self.file_database.clone()
    }

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
    }

    pub fn set_result(&mut self, results: Vec<FuzzyResult>) {
        self.current_result = Some(results);
    }

    pub fn reset_result(&mut self) {
        self.current_result = None;
    }

    pub fn reset_all(&mut self) {
        self.current_result = None;
        self.state = None;
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
    }
}

impl<'a> OverlayWidget for FuzzySearchWidget<'a> {
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

impl<'a> Widget for FuzzySearchWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
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
                    let mut last = 0;
                    for &pos in s.hits.iter() {
                        text.push(Span::raw(name[last..pos].to_string()));
                        text.push(Span::styled(
                            name[pos..pos + 1].to_string(),
                            self.style.yellow(),
                        ));
                        last = pos + 1;
                    }
                    text.push(Span::raw(name[last..].to_string()));
                    ListItem::new(Line::from(text))
                })
                .collect(),
            None => Vec::default(),
        };

        let search_list = List::new(search_result).gray().block(
            Block::default()
                .style(self.style)
                .title("Results")
                .borders(Borders::TOP)
                .padding(Padding::new(1, 1, 1, 1)),
        );

        let input_field = self.input_field.clone();
        outer_block.render(area, buf);
        input_field.widget().render(layout[0], buf);
        search_list.render(layout[1], buf);
    }
}
