use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::OverlayWidget;

#[derive(Debug, Default, Clone)]
pub struct OptionsWidget {
    percent_x: u8,
    percent_y: u8,
}

impl OptionsWidget {
    pub fn new() -> Self {
        Self {
            percent_x: 30,
            percent_y: 20,
        }
    }
}

impl OverlayWidget for OptionsWidget {
    fn area(&self, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(100 - self.percent_y as u16),
                    Constraint::Percentage(self.percent_y as u16),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(100 - self.percent_x as u16),
                    Constraint::Percentage(self.percent_x as u16),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl Widget for OptionsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let options_block = Block::default().title("Options").borders(Borders::ALL);

        let options_overlay = Paragraph::new(vec![
            Line::from(vec![Span::raw(" h Show help")]),
            Line::from(vec![Span::raw(" l Open login")]),
            Line::from(vec![Span::raw(" f Open fuzzy search")]),
        ])
        .block(options_block);

        options_overlay.render(area, buf);
    }
}
