use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};
use tui_textarea::Input;

use super::TextAreaWrapper;
use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

pub struct CommandInputWidget;

#[derive(Debug, Clone)]
pub struct CommandInputWidgetState {
    input_field: TextAreaWrapper,
    active: bool,
    theme: ThemeWrapper,
}

impl ThemedWidget for CommandInputWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl CommandInputWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            input_field: TextAreaWrapper::borderless(theme),
            theme: ThemeWrapper::new(theme),
            active: false,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.input_field = TextAreaWrapper::borderless(self.theme.inner());
        self.active = false;
    }
}

impl StatefulWidget for CommandInputWidget {
    type State = CommandInputWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let prefix = match state.active {
            true => ":".to_string(),
            false => "".to_string(),
        };

        let horizontal_split =
            Layout::horizontal([Constraint::Length(1), Constraint::Min(2)].as_ref()).split(area);

        let input_field = state
            .input_field
            .with_style(state.theme.inner())
            .with_placeholder("Enter your command");
        input_field.highlight(state.theme.base(), state.theme.highlight());

        let prefix_block = Paragraph::new(prefix).style(state.theme.style());
        prefix_block.render(horizontal_split[0], buf);
        input_field.render(horizontal_split[1], buf);
    }
}
