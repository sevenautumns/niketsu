use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::Style;

use super::chat::Chat;
use super::playlist::Playlist;
use super::{EventHandler, MainEventHandler, State};
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct ChatInput;

impl EventHandler for ChatInput {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Enter => {
                        let msg = view.app.chat_input_widget.get_input();
                        view.model.send_message(msg);
                        view.app.chat_input_widget.reset();
                    }
                    _ => view.app.chat_input_widget.input(*key),
                }
            }
        }
    }
}

impl MainEventHandler for ChatInput {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Up => view.transition(State::from(Chat {})),
            KeyCode::Right => view.transition(State::from(Playlist {})),
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.chat_input_widget.set_style(style);
    }
}
