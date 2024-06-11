use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::Style;

use super::chat_input::ChatInput;
use super::database::Database;
use super::playlist::Playlist;
use super::{EventHandler, MainEventHandler, State};
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Users;

impl EventHandler for Users {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Up => {
                        view.app.users_widget_state.next();
                    }
                    KeyCode::Down => {
                        view.app.users_widget_state.previous();
                    }
                    _ => {}
                }
            }
        }
    }
}

impl MainEventHandler for Users {
    fn handle_next(&self, view: &mut RatatuiView, event: &KeyEvent) {
        match event.code {
            KeyCode::Up => view.transition(State::from(Database {})),
            KeyCode::Down => view.transition(State::from(Playlist {})),
            KeyCode::Left => view.transition(State::from(ChatInput {})),
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.users_widget_state.set_style(style);
    }
}
