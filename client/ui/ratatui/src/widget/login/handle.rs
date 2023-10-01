use crossterm::event::{Event, KeyCode, KeyEventKind};
use niketsu_core::ui::{RoomChange, ServerChange};

use super::State;
use crate::view::RatatuiView;

pub fn handle_login(view: &mut RatatuiView, event: &Event) {
    if let Event::Key(key) = event {
        if key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Esc => view.app.reset_overlay(),
                KeyCode::Up => view.app.login_widget.previous_state(),
                KeyCode::Down => view.app.login_widget.next_state(),
                KeyCode::Char(' ') => match view.app.login_widget.current_state {
                    State::Address => {
                        view.app.login_widget.address_field.input(*key);
                    }
                    State::Username => {
                        view.app.login_widget.username_field.input(*key);
                    }
                    State::Room => {
                        view.app.login_widget.room_field.input(*key);
                    }
                    State::Password => {
                        view.app.login_widget.password_field.input(*key);
                    }
                    State::Secure => view.app.login_widget.secure = !view.app.login_widget.secure,
                },
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
                _ => match view.app.login_widget.current_state {
                    State::Address => {
                        view.app.login_widget.address_field.input(*key);
                    }
                    State::Username => {
                        view.app.login_widget.username_field.input(*key);
                    }
                    State::Room => {
                        view.app.login_widget.room_field.input(*key);
                    }
                    State::Password => {
                        view.app.login_widget.password_field.input(*key);
                    }
                    State::Secure => {}
                },
            }
        }
    }
}
