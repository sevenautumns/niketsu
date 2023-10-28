use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::view::{Mode, RatatuiView};

pub fn handle_command_prompt(view: &mut RatatuiView, event: &Event) {
    if let Event::Key(key) = event {
        match key.kind == KeyEventKind::Press {
            true => match key.code {
                KeyCode::Esc => {
                    view.app.set_mode(Mode::Normal);
                    view.app.command_input_widget.reset();
                }
                KeyCode::Enter => {
                    view.app.set_mode(Mode::Normal);
                    let msg = view.app.command_input_widget.get_input();
                    view.parse_commands(msg);
                    view.app.command_input_widget.reset();
                }
                _ => view.app.command_input_widget.input(*key),
            },
            false => (),
        }
    }
}
