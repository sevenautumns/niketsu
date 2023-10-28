use ratatui::prelude::{Buffer, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, StatefulWidget, Widget};
use ratatui_textarea::Input;

use super::TextAreaWrapper;

#[derive(Debug, Default, Clone)]
pub struct ChatInputWidget;

#[derive(Debug, Default, Clone)]
pub struct ChatInputWidgetState {
    input_field: TextAreaWrapper,
    style: Style,
}

impl ChatInputWidgetState {
    pub fn new() -> Self {
        let mut widget = Self::default();
        widget.setup_input_field();
        widget
    }

    fn setup_input_field(&mut self) {
        self.input_field
            .set_textarea_style(self.style, self.style.dark_gray().on_white());
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.input_field = TextAreaWrapper::default();
        self.setup_input_field();
    }
}

impl StatefulWidget for ChatInputWidget {
    type State = ChatInputWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let mut input_block = state.input_field.clone();
        input_block.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Message here")
                .style(state.style),
        );
        input_block.widget().render(area, buf);
    }
}
