use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::EventHandler;
use crate::view::{Mode, RatatuiView};

#[derive(Debug, Clone, Copy)]
pub struct Command;

impl EventHandler for Command {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            match key.kind == KeyEventKind::Press {
                true => match key.code {
                    KeyCode::Esc => {
                        view.app.set_mode(Mode::Normal);
                        view.app.command_input_widget.reset();
                    }
                    KeyCode::Enter => {
                        view.app.set_mode(Mode::Normal);
                        let msg = view.app.command_input_widget.get_input();
                        view.parse_commands(msg);
                        view.app.command_input_widget.reset();
                    }
                    _ => view.app.command_input_widget.input(*key),
                },
                false => (),
            }
        }
    }
}
