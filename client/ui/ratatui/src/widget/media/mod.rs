use delegate::delegate;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Padding, StatefulWidget, Widget};
use tui_textarea::Input;

use super::nav::ListNavigationState;
use super::{OverlayWidgetState, TextAreaWrapper};
use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

pub struct MediaDirWidget;

#[derive(Debug, Clone)]
pub struct MediaDirWidgetState {
    media_paths: Vec<String>,
    input_field: TextAreaWrapper,
    nav_state: ListNavigationState,
    theme: ThemeWrapper,
}

impl ThemedWidget for MediaDirWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl MediaDirWidgetState {
    pub fn new(paths: Vec<String>, theme: Theme) -> Self {
        let mut widget = Self {
            media_paths: paths,
            input_field: TextAreaWrapper::borderless(theme),
            nav_state: Default::default(),
            theme: ThemeWrapper::new(theme),
        };
        widget.select(Some(0));
        widget.update_len();
        widget
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
        self.input_field = TextAreaWrapper::borderless(self.theme.inner());
    }

    pub fn remove_path(&mut self) {
        if let Some(i) = self.selected()
            && !self.media_paths.is_empty()
            && i < self.media_paths.len()
        {
            _ = self.media_paths.remove(i);
        }
    }

    pub fn reset_all(&mut self) {
        self.select(Some(0));
        self.input_field = TextAreaWrapper::bordered(self.theme.inner());
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
        self.extended_area(r)
    }
}

impl StatefulWidget for MediaDirWidget {
    type State = MediaDirWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let style = state.theme.style();
        let hightlight_style = state.theme.highlight();

        let outer_block = Block::default()
            .title("Path")
            .padding(Padding::new(1, 1, 1, 1))
            .borders(Borders::ALL)
            .style(style);

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(3),
                ]
                .as_ref(),
            )
            .horizontal_margin(1)
            .split(area);

        let media_dirs: Vec<ListItem> = state
            .media_paths
            .iter()
            .map(|m| ListItem::new(Line::from(m.to_string()).style(style)))
            .collect();

        let media_path_list = List::new(media_dirs)
            .gray()
            .block(
                Block::default()
                    .style(style)
                    .title("Media Directories")
                    .borders(Borders::TOP)
                    .padding(Padding::new(1, 1, 0, 0)),
            )
            .highlight_style(hightlight_style)
            .highlight_symbol("> ");

        let input_field = state
            .input_field
            .with_style(state.theme.inner())
            .with_placeholder("Enter a path separated by /");
        input_field.highlight(state.theme.base(), hightlight_style);

        outer_block.render(area, buf);
        input_field.render(layout[1], buf);
        StatefulWidget::render(media_path_list, layout[2], buf, state.nav_state.inner());
    }
}
