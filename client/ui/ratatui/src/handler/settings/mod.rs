use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::Clear;

use super::{EventHandler, RenderHandler};
use crate::view::{App, RatatuiView};
use crate::widget::OverlayWidgetState;
use crate::widget::settings::SettingsWidget;

#[derive(Debug, Clone, Copy)]
pub struct Settings;

impl EventHandler for Settings {
    fn handle(&self, view: &mut RatatuiView, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Esc => view.app.reset_overlay(),
                    KeyCode::Up => view.app.settings_widget_state.previous_state(),
                    KeyCode::Down => view.app.settings_widget_state.next_state(),
                    KeyCode::Char(' ') => view.app.settings_widget_state.handle_input(*key),
                    KeyCode::Enter => {
                        view.app.reset_overlay();
                        let (relay, port, auto_connect, auto_share) =
                            view.app.settings_widget_state.collect_input();
                        view.save_settings(
                            relay.clone(),
                            port,
                            auto_connect,
                            auto_share,
                            view.app.settings_widget_state.theme_selection(),
                        );
                        view.handle_settings_change(relay, port, auto_connect, auto_share);
                    }
                    _ => {
                        if key.modifiers == KeyModifiers::CONTROL
                            && matches!(key.code, KeyCode::Char('d'))
                        {
                            view.reset_settings();
                            return;
                        }
                        match key.code {
                            KeyCode::Left => {
                                view.app.settings_widget_state.next_theme();
                            }
                            KeyCode::Right => {
                                view.app.settings_widget_state.previous_theme();
                            }
                            _ => {}
                        }
                        view.app.settings_widget_state.handle_input(*key);
                    }
                }
            }
        }
    }
}

impl RenderHandler for Settings {
    fn render(&self, frame: &mut ratatui::Frame, app: &mut App) {
        let area = app.settings_widget_state.area(frame.area());
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(SettingsWidget {}, area, &mut app.settings_widget_state);
    }
}
