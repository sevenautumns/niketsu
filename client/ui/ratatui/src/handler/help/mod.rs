use crossterm::event::{Event, KeyCode};
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::help::HelpWidget;

#[derive(Debug, Clone, Copy)]
pub struct Help;

impl EventHandler for Help {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Left | KeyCode::BackTab => view.app.help_widget_state.previous(),
                KeyCode::Right | KeyCode::Tab => view.app.help_widget_state.next(),
                _ => {
                    view.app.reset_overlay();
                }
            }
        }
    }
}

impl RenderHandler for Help {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.help_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(HelpWidget {}, area, &mut app.help_widget_state);
    }
}
