use arcstr::ArcStr;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use niketsu_core::room::RoomName;
use niketsu_core::ui::RoomChange;
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::login::LoginWidget;

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
                        let room = RoomName::from(input.0);
                        view.model.change_room(RoomChange {
                            room: room.clone(),
                            password: input.1.clone(),
                        });
                        let username: ArcStr = input.2.into();
                        view.model.change_username(username.clone());
                        view.save_config(input.1, room, username.clone());
                    }
                    _ => view.app.login_widget_state.input(*key),
                }
            }
        }
    }
}

impl RenderHandler for Login {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.login_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(LoginWidget {}, area, &mut app.login_widget_state);
    }
}
