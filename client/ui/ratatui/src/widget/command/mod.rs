use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};
use tui_textarea::Input;

use super::TextAreaWrapper;

pub struct CommandInputWidget;

#[derive(Debug, Default, Clone)]
pub struct CommandInputWidgetState {
    input_field: TextAreaWrapper,
    active: bool,
    style: Style,
}

impl CommandInputWidgetState {
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        self.input_field
            .set_textarea_style(self.style, self.style.dark_gray().on_white());
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.input_field = TextAreaWrapper::default();
        self.input_field.set_textarea_style(self.style, self.style);
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

        let horizontal_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(1)].as_ref())
            .split(area);

        let prefix_block = Paragraph::new(prefix);
        let input_block = state.input_field.widget();
        prefix_block.render(horizontal_split[0], buf);
        input_block.render(horizontal_split[1], buf);
    }
}
