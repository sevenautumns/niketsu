use ratatui::prelude::{Buffer, Constraint, Direction, Layout, Rect};
use ratatui::widgets::block::Block;
use ratatui::widgets::{Borders, Cell, Row, StatefulWidget, Table, Widget};

use super::OverlayWidgetState;

pub struct HelpWidget;

#[derive(Debug, Default, Clone)]
pub struct HelpWidgetState {
    items: Vec<Vec<&'static str>>,
}

impl HelpWidgetState {
    pub fn new() -> Self {
        Self {
            items: vec![
                vec!["Move left", "[Arrow Left]", "General"],
                vec!["Move right", "[Arrow Right]", "General"],
                vec!["Move up", "[Arrow Up]", "General"],
                vec!["Move down", "[Arrow]", "General"],
                vec!["Exit application", "q", "General"],
                vec!["Enter widget selection", "[Enter]", "General"],
                vec!["Enter command mode", ":", "General"],
                vec!["Enter options", "[Space]", "General"],
                vec!["Exit widget", "[Esc]", "General"],
                vec!["Enter command mode", ":", "Command"],
                vec!["Enter command input", "[Enter Key]", "Command"],
                vec!["Other keybindings", "[Emacs]", "InputFields"],
                vec!["Update file database", "[Enter]", "Database"],
                vec!["Cancel file database update", "[Backspace]", "Database"],
                vec!["Change room", "[Enter]", "Rooms"],
                vec!["Shift down selection", "x", "Playlist"],
                vec!["Delete selection", "d", "Playlist"],
                vec!["Paste selection", "p", "Playlist"],
                vec!["Enter login input", "[Enter]", "Login"],
                vec!["Change secure option", "[Space]", "Login"],
                vec!["Insert selection", "[Enter]", "FuzzySearch"],
                vec!["Add media path", "[Enter]", "Media"],
                vec!["Delete media path", "[Ctrl] + d", "Media"],
            ],
        }
    }
}

impl OverlayWidgetState for HelpWidgetState {
    fn area(&self, r: Rect) -> Rect {
        let vert_width = match r.height {
            0..=50 => 10,
            51..=100 => 20,
            _ => 40,
        };

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(vert_width),
                    Constraint::Percentage(100 - 2 * vert_width),
                    Constraint::Percentage(vert_width),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(vert_width),
                    Constraint::Percentage(100 - 2 * vert_width),
                    Constraint::Percentage(vert_width),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }
}

impl StatefulWidget for HelpWidget {
    type State = HelpWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let help_block = Block::default()
            .title("Help - press any key to exit")
            .borders(Borders::ALL);

        let header = Row::new(vec!["Description", "Control", "Context"]);

        let rows = state.items.iter().map(|line| {
            let items = line.iter().map(|r| Cell::from(*r));
            Row::new(items)
        });

        let table = Table::new(rows).header(header).block(help_block).widths(&[
            Constraint::Min(30),
            Constraint::Min(15),
            Constraint::Max(20),
        ]);
        Widget::render(table, area, buf);
    }
}
