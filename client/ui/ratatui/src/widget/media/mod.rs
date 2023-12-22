use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, List, ListItem, ListState, Padding, StatefulWidget, Widget};
use tui_textarea::Input;

use super::{ListStateWrapper, OverlayWidgetState, TextAreaWrapper};

pub struct MediaDirWidget;

#[derive(Debug, Default, Clone)]
pub struct MediaDirWidgetState {
    media_paths: Vec<String>,
    input_field: TextAreaWrapper,
    list_state: ListStateWrapper,
    style: Style,
}

impl MediaDirWidgetState {
    pub fn new(paths: Vec<String>) -> Self {
        let mut widget = Self {
            media_paths: paths,
            input_field: Default::default(),
            list_state: Default::default(),
            style: Default::default(),
        };
        widget.setup_input_field();
        widget.list_state.select(Some(0));
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

    pub fn get_paths(&self) -> Vec<String> {
        self.media_paths.clone()
    }

    pub fn get_state(&self) -> ListState {
        self.list_state.clone_inner()
    }

    pub fn push_path(&mut self) {
        let path = self.input_field.get_input();
        self.media_paths.push(path);
        self.input_field = TextAreaWrapper::from("".to_string());
        self.setup_input_field();
    }

    pub fn remove_path(&mut self) {
        if let Some(i) = self.list_state.selected() {
            if self.media_paths.is_empty() {
                _ = self.media_paths.remove(i);
            }
        }
    }

    pub fn reset_all(&mut self) {
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
        self.media_paths.len()
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }
}

impl OverlayWidgetState for MediaDirWidgetState {
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

impl StatefulWidget for MediaDirWidget {
    type State = MediaDirWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let outer_block = Block::default()
            .title("Search")
            .borders(Borders::ALL)
            .gray();

        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .margin(1)
            .split(area);

        let media_dirs: Vec<ListItem> = state
            .media_paths
            .iter()
            .map(|m| ListItem::new(Line::from(m.to_string())))
            .collect();

        let media_path_list = List::new(media_dirs)
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
        StatefulWidget::render(media_path_list, layout[1], buf, state.list_state.inner());
    }
}
