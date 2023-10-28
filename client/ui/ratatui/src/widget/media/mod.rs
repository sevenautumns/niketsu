use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Padding, StatefulWidget, Widget,
};
use ratatui_textarea::Input;

use super::{ListStateWrapper, OverlayWidgetState, TextAreaWrapper};

#[derive(Debug, Default, Clone)]
pub struct MediaDirWidget {
    media_paths: Vec<String>,
    input_field: TextAreaWrapper,
    state: ListStateWrapper,
    style: Style,
}

impl MediaDirWidget {
    pub fn new(paths: Vec<String>) -> Self {
        let mut widget = Self {
            media_paths: paths,
            input_field: Default::default(),
            state: Default::default(),
            style: Default::default(),
        };
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

    pub fn get_paths(&self) -> Vec<String> {
        self.media_paths.clone()
    }

    pub fn get_state(&self) -> ListState {
        self.state.clone_inner()
    }

    pub fn push_path(&mut self) {
        let path = self.input_field.get_input();
        self.media_paths.push(path);
        self.input_field = TextAreaWrapper::from("".to_string());
        self.setup_input_field();
    }

    pub fn remove_path(&mut self) {
        if let Some(i) = self.state.selected() {
            _ = self.media_paths.remove(i);
        }
    }

    pub fn reset_all(&mut self) {
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
        self.media_paths.len()
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }
}

impl OverlayWidgetState for MediaDirWidget {
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

        let media_paths = self.media_paths.clone();
        let media_dirs: Vec<ListItem> = media_paths
            .into_iter()
            .map(|m| ListItem::new(Line::from(m)))
            .collect();

        let media_path_list = List::new(media_dirs)
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
        StatefulWidget::render(media_path_list, layout[1], buf, state);
    }
}
