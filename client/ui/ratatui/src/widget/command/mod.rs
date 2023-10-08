use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Paragraph, Widget};
use ratatui_textarea::Input;

use super::TextAreaWrapper;

#[derive(Debug, Default, Clone)]
pub struct CommandInputWidget {
    state: CommandInputWidgetState,
}

#[derive(Debug, Default, Clone)]
pub struct CommandInputWidgetState {
    input_field: TextAreaWrapper,
    active: bool,
    style: Style,
}

impl CommandInputWidget {
    pub fn set_active(&mut self, active: bool) {
        self.state.active = active;
        self.state
            .input_field
            .set_textarea_style(self.state.style, self.state.style.dark_gray().on_white());
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.state.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.state.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.state.input_field = TextAreaWrapper::default();
        self.state
            .input_field
            .set_textarea_style(self.state.style, self.state.style);
        self.state.active = false;
    }
}

impl Widget for CommandInputWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let prefix = match self.state.active {
            true => ":".to_string(),
            false => "".to_string(),
        };

        let horizontal_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(1), Constraint::Min(1)].as_ref())
            .split(area);

        let prefix_block = Paragraph::new(prefix);
        let input_block = self.state.input_field.widget();
        prefix_block.render(horizontal_split[0], buf);
        input_block.render(horizontal_split[1], buf);
    }
}
