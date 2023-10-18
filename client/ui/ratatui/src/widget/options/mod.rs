use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, StatefulWidget, Widget};

use super::OverlayWidgetState;

pub struct OptionsWidget;

#[derive(Debug, Default, Clone)]
pub struct OptionsWidgetState {
    percent_x: u8,
    percent_y: u8,
    style: Style,
}

impl OptionsWidgetState {
    pub fn new() -> Self {
        Self {
            percent_x: 30,
            percent_y: 20,
            style: Style::default(),
        }
    }
}

impl OverlayWidgetState for OptionsWidgetState {
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

impl StatefulWidget for OptionsWidget {
    type State = OptionsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let options_block = Block::default().title("Options").borders(Borders::ALL);

        let options_overlay = Paragraph::new(vec![
            Line::from(vec![Span::raw(" h Show help")]),
            Line::from(vec![Span::raw(" l Open login")]),
            Line::from(vec![Span::raw(" f Open fuzzy search")]),
            Line::from(vec![Span::raw(" m Open media paths")]),
        ])
        .style(state.style)
        .block(options_block);

        options_overlay.render(area, buf);
    }
}
