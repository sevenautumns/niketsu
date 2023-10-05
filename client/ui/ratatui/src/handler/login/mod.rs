use crossterm::event::{Event, KeyCode, KeyEventKind};
use niketsu_core::ui::{RoomChange, ServerChange};

use super::EventHandler;
use crate::view::RatatuiView;

#[derive(Debug, Clone, Copy)]
pub struct Login;

impl EventHandler for Login {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => view.app.reset_overlay(),
                    KeyCode::Up => view.app.login_widget.previous_state(),
                    KeyCode::Down => view.app.login_widget.next_state(),
                    KeyCode::Char(' ') => view.app.login_widget.input(*key),
                    KeyCode::Enter => {
                        view.app.reset_overlay();
                        let input = view.app.login_widget.collect_input();
                        view.model.change_server(ServerChange {
                            addr: input.0,
                            secure: input.3,
                            password: Some(input.1),
                            room: RoomChange { room: input.2 },
                        });
                    }
                    _ => view.app.login_widget.input(*key),
                }
            }
        }
    }
}
