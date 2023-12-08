use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget};

use super::OverlayWidgetState;

pub struct OptionsWidget;

#[derive(Debug, Default, Clone)]
pub struct OptionsWidgetState {
    style: Style,
}

impl OverlayWidgetState for OptionsWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(10)].as_ref())
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(30)].as_ref())
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for OptionsWidget {
    type State = OptionsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let options_block = Block::default().title("Options").borders(Borders::ALL);

        let options_overlay = Paragraph::new(vec![
            Line::from(vec![Span::raw(" h     Show help")]),
            Line::from(vec![Span::raw(" l     Open login")]),
            Line::from(vec![Span::raw(" f     Open fuzzy search")]),
            Line::from(vec![Span::raw(" m     Open media paths")]),
            Line::from(vec![Span::raw(" r     Toggle ready")]),
        ])
        .style(state.style)
        .block(options_block);

        options_overlay.render(area, buf);
    }
}
