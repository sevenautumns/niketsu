use std::cmp::max;

use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget, Wrap};

use crate::widget::OverlayWidgetState;

pub struct VideoNameWidget;

#[derive(Debug, Default, Clone)]
pub struct VideoNameWidgetState {
    name: String,
}

impl VideoNameWidgetState {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

impl OverlayWidgetState for VideoNameWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let mut name_len = self.name.len() as u16;
        name_len = max(name_len.saturating_add(2), 18);

        let width_percent = match r.height {
            0..=50 => 0.9,
            51..=100 => 0.8,
            _ => 0.6,
        };
        let width = (width_percent * r.width as f32) as u16;
        let width_sides = ((100.0 * (1.0 - width_percent)) / 2.0) as u16;
        let length = max(name_len / width + 3, 3);
        let length_sides = (r.height - length) / 2;

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Max(length_sides),
                    Constraint::Length(length),
                    Constraint::Min(length_sides),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(width_sides),
                    Constraint::Min(width),
                    Constraint::Percentage(width_sides),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for VideoNameWidget {
    type State = VideoNameWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let video_block = Block::default()
            .title("Full video name")
            .borders(Borders::ALL);

        let video = Paragraph::new(state.name.clone())
            .block(video_block)
            .wrap(Wrap { trim: false });
        video.render(area, buf);
    }
}
