use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::{Color, Style};

use super::chat_input::ChatInput;
use super::database::Database;
use super::{MainEventHandler, State};
use crate::handler::EventHandler;
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Chat;

impl EventHandler for Chat {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Down => view.app.chat_widget_state.previous(),
                    KeyCode::Up => view.app.chat_widget_state.next(),
                    KeyCode::PageUp => view.app.chat_widget_state.jump_next(5),
                    KeyCode::PageDown => view.app.chat_widget_state.jump_previous(5),
                    KeyCode::Home => view.app.chat_widget_state.jump_start(),
                    KeyCode::End => view.app.chat_widget_state.jump_end(),
                    _ => {}
                }
            }
        }
    }
}

impl MainEventHandler for Chat {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Right => {
                view.app.chat_widget_state.set_style(Style::default());
                view.app
                    .database_widget_state
                    .set_style(Style::default().fg(Color::Magenta));
                view.app.set_current_state(State::from(Database {}));
            }
            KeyCode::Down => {
                view.app.chat_widget_state.set_style(Style::default());
                view.app.set_current_state(State::from(ChatInput {}));
                view.app
                    .chat_input_widget
                    .set_style(Style::default().fg(Color::Magenta));
            }
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.chat_widget_state.set_style(style);
    }
}
