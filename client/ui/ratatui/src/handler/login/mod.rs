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
                    KeyCode::Up => view.app.login_widget_state.previous_state(),
                    KeyCode::Down => view.app.login_widget_state.next_state(),
                    KeyCode::Char(' ') => view.app.login_widget_state.input(*key),
                    KeyCode::Enter => {
                        view.app.reset_overlay();
                        let input = view.app.login_widget_state.collect_input();
                        view.model.change_server(ServerChange {
                            addr: input.0.clone(),
                            secure: input.1,
                            password: Some(input.2.clone()),
                            room: RoomChange {
                                room: input.3.clone(),
                            },
                        });
                        view.model.change_username(input.4.clone());
                        view.save_config(input.0, input.1, input.2, input.3, input.4);
                    }
                    _ => view.app.login_widget_state.input(*key),
                }
            }
        }
    }
}
