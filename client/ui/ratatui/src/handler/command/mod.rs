use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::EventHandler;
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct Command;

impl EventHandler for Command {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            match key.kind == KeyEventKind::Press {
                true => match key.code {
                    KeyCode::Esc => {
                        view.app.reset_overlay();
                        view.app.command_input_widget_state.reset();
                    }
                    KeyCode::Enter => {
                        view.app.reset_overlay();
                        let msg = view.app.command_input_widget_state.get_input();
                        view.parse_commands(msg);
                        view.app.command_input_widget_state.reset();
                    }
                    _ => view.app.command_input_widget_state.input(*key),
                },
                false => (),
            }
        }
    }
}
