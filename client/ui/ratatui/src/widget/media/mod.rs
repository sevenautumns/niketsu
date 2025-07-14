use delegate::delegate;
use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, List, ListItem, Padding, StatefulWidget, Widget};
use tui_textarea::Input;

use super::nav::ListNavigationState;
use super::{OverlayWidgetState, TextAreaWrapper};

pub struct MediaDirWidget;

#[derive(Debug, Default, Clone)]
pub struct MediaDirWidgetState {
    media_paths: Vec<String>,
    input_field: TextAreaWrapper,
    nav_state: ListNavigationState,
    style: Style,
}

impl MediaDirWidgetState {
    pub fn new(paths: Vec<String>) -> Self {
        let mut widget = Self {
            media_paths: paths,
            input_field: Default::default(),
            nav_state: Default::default(),
            style: Default::default(),
        };
        widget.setup_input_field();
        widget.select(Some(0));
        widget.update_len();
        widget
    }

    fn setup_input_field(&mut self) {
        self.input_field = TextAreaWrapper::default();
        self.input_field
            .with_block(
                Block::default()
                    .borders(Borders::NONE)
                    .padding(Padding::new(1, 0, 0, 0)),
            )
            .with_placeholder("Enter a path separated by /")
            .highlight(Style::default(), self.style.dark_gray().on_white());
    }

    fn update_len(&mut self) {
        self.nav_state.set_list_len(self.media_paths.len());
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn get_paths(&self) -> Vec<String> {
        self.media_paths.clone()
    }

    pub fn push_path(&mut self) {
        let path = self.input_field.get_input();
        self.media_paths.push(path);
        self.update_len();
        self.setup_input_field();
    }

    pub fn remove_path(&mut self) {
        if let Some(i) = self.selected() {
            if !self.media_paths.is_empty() && i < self.media_paths.len() {
                _ = self.media_paths.remove(i);
            }
        }
    }

    pub fn reset_all(&mut self) {
        self.select(Some(0));
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
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
        }
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
        let outer_block = Block::default().title("Path").borders(Borders::ALL).gray();

        let layout = Layout::default()
            .constraints([Constraint::Length(1), Constraint::Min(3)].as_ref())
            .horizontal_margin(1)
            .vertical_margin(1)
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
                    .title("Media Directories")
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 0, 0, 1)),
            )
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        outer_block.render(area, buf);
        state.input_field.render(layout[0], buf);
        StatefulWidget::render(media_path_list, layout[1], buf, state.nav_state.inner());
    }
}
