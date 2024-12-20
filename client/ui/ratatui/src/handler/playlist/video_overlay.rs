use crossterm::event::{Event, KeyEventKind};

use super::EventHandler;
use crate::view::RatatuiView;

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
