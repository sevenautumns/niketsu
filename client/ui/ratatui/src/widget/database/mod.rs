use niketsu_core::file_database::FileStore;
use ratatui::prelude::{Buffer, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, Borders, Gauge, StatefulWidget, Widget};

pub struct DatabaseWidget;

#[derive(Debug, Default, Clone)]
pub struct DatabaseWidgetState {
    file_database: FileStore,
    file_database_status: u16,
    style: Style,
}

impl DatabaseWidgetState {
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn set_file_database(&mut self, file_database: FileStore) {
        self.file_database = file_database;
    }

    pub fn set_file_database_status(&mut self, status: u16) {
        self.file_database_status = status;
    }
}

impl StatefulWidget for DatabaseWidget {
    type State = DatabaseWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let gauge_block = Block::default()
            .style(state.style)
            .title(Title::from("Database"))
            .borders(Borders::ALL);

        let num_files = format!("{len} files loaded", len = state.file_database.len());
        let gauge = Gauge::default()
            .block(gauge_block)
            .gauge_style(Style::default().fg(Color::Green))
            .percent(state.file_database_status)
            .label(num_files.fg(Color::DarkGray));

        gauge.render(area, buf);
    }
}
