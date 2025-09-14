use niketsu_core::file_database::FileStore;
use ratatui::prelude::{Buffer, Rect};
use ratatui::style::Stylize;
use ratatui::text::Span;
use ratatui::widgets::block::{Block, Title};
use ratatui::widgets::{Borders, Gauge, Padding, StatefulWidget, Widget};

use crate::theme::{Theme, ThemeWrapper, ThemedWidget};

pub struct DatabaseWidget;

#[derive(Debug, Default, Clone)]
pub struct DatabaseWidgetState {
    file_database: FileStore,
    file_database_status: u16,
    theme: ThemeWrapper,
}

impl ThemedWidget for DatabaseWidgetState {
    fn theme(&mut self) -> &mut ThemeWrapper {
        &mut self.theme
    }
}

impl DatabaseWidgetState {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme: ThemeWrapper::new(theme),
            ..Default::default()
        }
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
            .style(state.theme.style())
            .title(Title::from("Database"))
            .padding(Padding::horizontal(2))
            .borders(Borders::ALL);

        let num_files = format!("{len} files loaded", len = state.file_database.len());
        //TODO: this might need to change depending on what color the gauge is
        // some colors are not visible on green backgrounds ...
        let paragraph = Span::raw(num_files).style(state.theme.style().black().on_green());
        let gauge = Gauge::default()
            .block(gauge_block)
            .gauge_style(state.theme.style().green())
            .percent(state.file_database_status)
            .label(paragraph);

        gauge.render(area, buf);
    }
}
