use ratatui::prelude::{Buffer, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Borders, Widget};
use ratatui_textarea::Input;

use super::TextAreaWrapper;

#[derive(Debug, Default, Clone)]
pub struct ChatInputWidget<'a> {
    state: ChatInputWidgetState<'a>,
}

#[derive(Debug, Default, Clone)]
pub struct ChatInputWidgetState<'a> {
    input_field: TextAreaWrapper<'a>,
    style: Style,
}

impl<'a> ChatInputWidget<'a> {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget
    }

    fn setup_input_field(&mut self) {
        self.state
            .input_field
            .set_textarea_style(self.state.style, self.state.style.dark_gray().on_white());
    }

    pub fn set_style(&mut self, style: Style) {
        self.state.style = style;
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.state.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.state.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.state.input_field = TextAreaWrapper::default();
        self.setup_input_field();
    }
}

//TODO wrap cursor
impl<'a> Widget for ChatInputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut input_block = self.state.input_field.clone();
        input_block.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Message here")
                .style(self.state.style),
        );
        input_block.widget().render(area, buf);
    }
}
