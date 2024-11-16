use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::command::CommandInputWidget;

#[derive(Debug, Clone, Copy)]
pub struct Command;

impl EventHandler for Command {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
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
                }
            }
        }
    }
}

impl RenderHandler for Command {
    fn render(&self, frame: &mut Frame, app: &mut App) {
        let area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(frame.area());

        frame.render_stateful_widget(
            CommandInputWidget {},
            area[1],
            &mut app.command_input_widget_state,
        );
    }
}
