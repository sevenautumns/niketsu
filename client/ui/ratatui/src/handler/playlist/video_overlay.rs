use crossterm::event::{Event, KeyEventKind};
use ratatui::widgets::Clear;

use super::EventHandler;
use crate::handler::RenderHandler;
use crate::view::{App, RatatuiView};
use crate::widget::playlist::video_overlay::VideoNameWidget;
use crate::widget::OverlayWidgetState;

#[derive(Debug, Clone, Copy)]
pub struct VideoName;

impl EventHandler for VideoName {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                view.app.reset_overlay();
            }
        }
    }
}

impl RenderHandler for VideoName {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.video_name_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(VideoNameWidget {}, area, &mut app.video_name_widget_state);
    }
}
