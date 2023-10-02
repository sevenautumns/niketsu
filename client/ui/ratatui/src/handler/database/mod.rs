use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::style::Style;

use super::chat::Chat;
use super::room::Rooms;
use super::{MainEventHandler, State};
use crate::handler::EventHandler;
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Database;

impl EventHandler for Database {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.hover_highlight();
                    }
                    KeyCode::Enter => view.model.start_db_update(),
                    KeyCode::Backspace => view.model.stop_db_update(),
                    _ => {}
                }
            }
        }
    }
}

impl MainEventHandler for Database {
    fn handle_next(&self, view: &mut RatatuiView, event: &crossterm::event::KeyEvent) {
        match event.code {
            KeyCode::Left => view.transition(State::from(Chat {})),
            KeyCode::Down => view.transition(State::from(Rooms {})),
            _ => {}
        }
    }

    fn set_style(&self, view: &mut RatatuiView, style: Style) {
        view.app.database_widget_state.set_style(style);
    }
}
