use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, StatefulWidget, Widget};
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
        self.input_field.highlight(self.style, self.style.cyan());
        self.input_field
            .with_default_style()
            .with_block(
                Block::default()
                    .borders(Borders::NONE)
                    .padding(Padding::new(0, 0, 0, 0)),
            )
            .with_placeholder("Enter your command");
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.input_field = TextAreaWrapper::default();
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
            .constraints([Constraint::Length(1), Constraint::Min(2)].as_ref())
            .split(area);

        let prefix_block = Paragraph::new(prefix);
        prefix_block.render(horizontal_split[0], buf);
        state.input_field.widget().render(horizontal_split[1], buf);
    }
}
