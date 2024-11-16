use crossterm::event::{Event, KeyEventKind};
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::help::HelpWidget;
use crate::widget::OverlayWidgetState;

#[derive(Debug, Clone, Copy)]
pub struct Help;

impl EventHandler for Help {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                view.app.reset_overlay();
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
