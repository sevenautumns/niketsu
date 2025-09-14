use ratatui::prelude::{Buffer, Rect};
use ratatui::widgets::StatefulWidget;
use tui_textarea::Input;

use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

use super::TextAreaWrapper;

#[derive(Debug, Default, Clone)]
pub struct ChatInputWidget;

#[derive(Debug, Clone)]
pub struct ChatInputWidgetState {
    input_field: TextAreaWrapper,
    theme: ThemeWrapper,
}

impl ThemedWidget for ChatInputWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl ChatInputWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme: ThemeWrapper::new(theme),
            input_field: TextAreaWrapper::new(Some("Message here".to_string()), None, theme, true),
        }
    }

    pub fn input(&mut self, event: impl Into<Input>) {
        self.input_field.input(event);
    }

    pub fn get_input(&self) -> String {
        self.input_field.get_input()
    }

    pub fn reset(&mut self) {
        self.input_field = TextAreaWrapper::bordered(self.theme.inner());
    }
}

impl StatefulWidget for ChatInputWidget {
    type State = ChatInputWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let input_field = state
            .input_field
            .with_style(state.theme.inner())
            .with_placeholder("Write a message");
        input_field.highlight(state.theme.style(), state.theme.style());

        input_field.render(area, buf);
    }
}
