use ratatui::layout::Flex;
use ratatui::prelude::{Buffer, Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Paragraph, StatefulWidget, Widget};

use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

use super::OverlayWidgetState;

pub struct OptionsWidget;

// width and height depend on the widest entry and the number of items
#[derive(Debug, Default, Clone)]
pub struct OptionsWidgetState {
    theme: ThemeWrapper,
    options: Text<'static>,
    width: usize,
    height: usize,
}

impl ThemedWidget for OptionsWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl OptionsWidgetState {
    pub fn new(theme: Theme) -> Self {
        let options = Text::from(vec![
            Line::from(vec![Span::raw(" h     Show help")]),
            Line::from(vec![Span::raw(" l     Open login")]),
            Line::from(vec![Span::raw(" c     Show settings")]),
            Line::from(vec![Span::raw(" /     Open search")]),
            Line::from(vec![Span::raw(" m     Open media paths")]),
            Line::from(vec![Span::raw(" b     Open playlist browser")]),
            Line::from(vec![Span::raw(" r     Toggle ready")]),
            Line::from(vec![Span::raw(" f     Toggle file share")]),
            Line::from(vec![Span::raw(" x     Start file request")]),
            Line::from(vec![Span::raw(" s     Start file db update")]),
            Line::from(vec![Span::raw(" p     Stop file db update")]),
        ]);
        // borders need to be considered
        let width = options.width().saturating_add(2);
        let height = options.height().saturating_add(1);
        Self {
            theme: ThemeWrapper::new(theme),
            options,
            width,
            height,
        }
    }
}

impl OverlayWidgetState for OptionsWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let [area] = Layout::vertical([Constraint::Length(self.height as u16)])
            .flex(Flex::End)
            .areas(r);

        let [popup_layout] = Layout::horizontal([Constraint::Length(self.width as u16)])
            .flex(Flex::End)
            .areas(area);
        popup_layout
    }
}

impl StatefulWidget for OptionsWidget {
    type State = OptionsWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let options_block = Block::default().title("Options").borders(Borders::ALL);

        let options_overlay = Paragraph::new(state.options.clone())
            .style(state.theme.style())
            .block(options_block);

        options_overlay.render(area, buf);
    }
}
