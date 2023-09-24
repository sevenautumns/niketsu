use std::io::{self, Stdout};

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, PlaylistVideo};
use niketsu_core::rooms::{RoomList, RoomName};
use niketsu_core::ui::{PlayerMessage, RoomChange, ServerChange, UiModel};
use niketsu_core::user::UserStatus;
use ratatui::prelude::*;
use ratatui::widgets::block::Title;
use ratatui::widgets::*;

pub struct RatatuiView {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    app: App,
    model: UiModel,
}

enum WidgetType {
    Chat,
    ChatInput,
    Database,
    Rooms,
    Playlist,
    Video,
}

enum OverlayType {
    Login,
    Option,
    Help,
}

#[derive(Clone)]
enum SelectedField {
    Address,
    Password,
}

impl Default for SelectedField {
    fn default() -> Self {
        Self::Address
    }
}

//TODO secure field
#[derive(Default, Clone)]
struct OverlayLoginWidget {
    address_field_state: InputWidgetState,
    password_field_state: InputWidgetState,
    selected: SelectedField,
    style: Style,
}

impl OverlayLoginWidget {
    fn new() -> Self {
        Self {
            address_field_state: Default::default(),
            password_field_state: Default::default(),
            selected: Default::default(),
            style: Style::default().fg(Color::Cyan),
        }
    }

    fn area(&self, r: Rect) -> Rect {
        // TODO smaller/larger depending on area
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(30),
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                ]
                .as_ref(),
            )
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(30),
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                ]
                .as_ref(),
            )
            .split(popup_layout[1])[1]
    }

    fn change_selected(&mut self) {
        match self.selected {
            SelectedField::Address => self.selected = SelectedField::Password,
            SelectedField::Password => self.selected = SelectedField::Address,
        }
    }

    fn cursor_position(&self) -> u16 {
        match self.selected {
            SelectedField::Address => self.address_field_state.cursor_position(),
            SelectedField::Password => self.password_field_state.cursor_position(),
        }
    }

    fn move_cursor_left(&mut self) {
        match self.selected {
            SelectedField::Address => self.address_field_state.move_cursor_left(),
            SelectedField::Password => self.password_field_state.move_cursor_left(),
        }
    }

    fn move_cursor_right(&mut self) {
        match self.selected {
            SelectedField::Address => self.address_field_state.move_cursor_right(),
            SelectedField::Password => self.password_field_state.move_cursor_right(),
        }
    }

    fn enter_char(&mut self, new_char: char) {
        match self.selected {
            SelectedField::Address => self.address_field_state.enter_char(new_char),
            SelectedField::Password => self.password_field_state.enter_char(new_char),
        }
    }

    fn delete_char(&mut self) {
        match self.selected {
            SelectedField::Address => self.address_field_state.delete_char(),
            SelectedField::Password => self.password_field_state.delete_char(),
        }
    }

    fn submit_message(&mut self) -> (String, String) {
        let address = self.address_field_state.submit_message();
        let password = self.password_field_state.submit_message();
        (address, password)
    }

    fn flush(&mut self) {
        self.address_field_state.flush();
        self.password_field_state.flush();
    }

    fn reset(&mut self) {
        self.flush();
        self.selected = SelectedField::Address;
    }
}

//TODO increase chat size if text exceeds size
//TODO wrap cursor
impl Widget for ChatInputWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let input_text = self.state.input.clone();
        let input_block = Paragraph::new(input_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Message here")
                    .style(self.style),
            )
            .wrap(Wrap { trim: true });
        input_block.render(area, buf);
    }
}
#[derive(Default, Clone)]
struct OverlayOptionsWidget {
    percent_x: u8,
    percent_y: u8,
}

impl OverlayOptionsWidget {
    fn new() -> Self {
        Self {
            percent_x: 30,
            percent_y: 20,
        }
    }

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

impl Widget for OverlayOptionsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let options_block = Block::default().title("Options").borders(Borders::ALL);

        let options_overlay = Paragraph::new(vec![
            Line::from(vec![Span::raw(" l Open login")]),
            Line::from(vec![Span::raw(" h Show help")]),
        ])
        .block(options_block);

        options_overlay.render(area, buf);
    }
}

//TODO highlight own user messages
#[derive(Default, Clone)]
struct ChatWidget {
    state: ChatWidgetState,
}

#[derive(Default, Clone)]
struct ChatWidgetState {
    vertical_scroll_state: ScrollbarState,
    state: ListState,
    user: UserStatus,
    style: Style,
    messages: Vec<PlayerMessage>,
}

impl ChatWidget {
    fn new() -> Self {
        ChatWidget {
            state: Default::default(),
        }
    }

    fn change_style(&mut self, style: Style) {
        self.state.style = style;
    }

    fn next(&mut self) {
        let i = match self.state.state.selected() {
            Some(i) => {
                if i == 0 {
                    0
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.state.selected() {
            Some(i) => {
                if i >= self.state.messages.len().saturating_sub(1) {
                    self.state.messages.len().saturating_sub(1)
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn update_cursor_latest(&mut self) {
        self.state
            .state
            .select(Some(self.state.messages.len().saturating_sub(1)));
        self.state.vertical_scroll_state = self
            .state
            .vertical_scroll_state
            .position(self.state.messages.len().saturating_sub(1) as u16);
    }
}

impl StatefulWidget for ChatWidget {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        //TODO colour schemes for different users?
        use niketsu_core::ui::MessageSource::*;
        let messages: Vec<ListItem> = self
            .state
            .messages
            .iter()
            .map(|t| {
                let head_message = format!(" at {}:", t.timestamp.format("%H:%M:%S"));
                let head_message = match &t.source {
                    UserMessage(user_name) => {
                        let name = match self.state.user.eq(user_name) {
                            true => Span::raw(user_name).italic().green(),
                            false => Span::raw(user_name),
                        };
                        let message = Span::raw(head_message).gray();
                        Line::from(vec![name, message])
                    }
                    UserAction(user_name) => {
                        let message = format!("User action of {user_name}{head_message}");
                        Line::from(Span::raw(message).light_magenta())
                    }
                    Server => {
                        let message = format!("Server notification{head_message}");
                        Line::from(Span::raw(message).light_red())
                    }
                    Internal => {
                        let message = format!("Internal notification{head_message}");
                        Line::from(Span::raw(message).red())
                    }
                };
                let tail_message = Line::from(Span::raw(t.message.clone()));
                ListItem::new(vec![head_message, tail_message])
            })
            .collect();

        let messages_block = Block::default()
            .style(self.state.style)
            .title(Title::from("Chat"))
            .borders(Borders::ALL);

        let messages_list = List::new(messages.clone())
            .gray()
            .block(messages_block)
            .highlight_style(Style::default().fg(Color::Cyan));

        StatefulWidget::render(messages_list, area, buf, state);

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        let mut state = self.state.vertical_scroll_state.clone();
        state = state.content_length(messages.len() as u16);
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }
}

#[derive(Default, Clone)]
struct DatabaseWidget {
    state: DatabaseWidgetState,
}

#[derive(Default, Clone)]
struct DatabaseWidgetState {
    file_database: FileStore,
    file_database_status: u16,
    style: Style,
}

impl DatabaseWidget {
    fn new() -> Self {
        DatabaseWidget {
            state: Default::default(),
        }
    }

    fn change_style(&mut self, style: Style) {
        self.state.style = style;
    }
}

impl Widget for DatabaseWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let gauge_block = Block::default()
            .style(self.state.style)
            .title(Title::from("Database"))
            .borders(Borders::ALL);

        let num_files = format!("{len} files loaded", len = self.state.file_database.len());
        let gauge = Gauge::default()
            .block(gauge_block)
            .gauge_style(Style::default().fg(Color::Green))
            .percent(self.state.file_database_status)
            .label(num_files.fg(Color::DarkGray));

        gauge.render(area, buf);
    }
}

#[derive(Default, Clone)]
struct PlaylistWidget {
    state: PlaylistWidgetState,
}

#[derive(Default, Clone)]
struct PlaylistWidgetState {
    vertical_scroll_state: ScrollbarState,
    playlist: Playlist,
    state: ListState,
    selection_offset: usize,
    clipboard: Option<Vec<PlaylistVideo>>,
    style: Style,
}

impl PlaylistWidget {
    fn new() -> Self {
        PlaylistWidget {
            state: Default::default(),
        }
    }

    fn change_style(&mut self, style: Style) {
        self.state.style = style;
    }

    fn next(&mut self) {
        self.state.selection_offset = 0;
        let i = match self.state.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.state.playlist.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn previous(&mut self) {
        self.state.selection_offset = 0;
        let i = match self.state.state.selected() {
            Some(i) => {
                if i >= self.state.playlist.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn unselect(&mut self) {
        self.state.state.select(None);
        self.reset_offset();
    }

    fn reset_offset(&mut self) {
        self.state.selection_offset = 0;
    }

    fn increase_selection_offset(&mut self) {
        if self.state.selection_offset < self.state.playlist.len() {
            self.state.selection_offset += 1;
        }
    }

    fn get_current_video(&self) -> Option<&PlaylistVideo> {
        match self.state.state.selected() {
            Some(index) => self.state.playlist.get(index).clone(),
            None => None,
        }
    }

    fn get_selected_videos(&mut self) -> Option<(usize, usize)> {
        match self.state.state.selected() {
            Some(index) => {
                self.state.clipboard = Some(
                    self.state
                        .playlist
                        .get_range(index, index + self.state.selection_offset)
                        .cloned()
                        .collect(),
                );
                Some((index, index + self.state.selection_offset))
            }
            None => None,
        }
    }
}

impl StatefulWidget for PlaylistWidget {
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let scroll_block = Block::default()
            .title(Title::from("Playlist"))
            .borders(Borders::ALL)
            .style(self.state.style);

        let playlist = self.state.playlist;
        //TODO calculate boundaries myself...
        let playlist: Vec<ListItem> = match state.selected() {
            Some(index) => playlist
                .iter()
                .take(index)
                .map(|t| ListItem::new(vec![Line::from(t.as_str().gray())]))
                .chain(
                    playlist
                        .iter()
                        .skip(index)
                        .take(self.state.selection_offset + 1)
                        .map(|t| ListItem::new(vec![Line::from(t.as_str().cyan())])),
                )
                .chain(
                    playlist
                        .iter()
                        .skip(index + self.state.selection_offset + 1)
                        .map(|t| ListItem::new(vec![Line::from(t.as_str().gray())])),
                )
                .collect(),
            None => playlist
                .iter()
                .map(|t| ListItem::new(vec![Line::from(t.as_str())]))
                .collect(),
        };

        let list = List::new(playlist.clone())
            .gray()
            .block(scroll_block)
            .highlight_style(Style::default().fg(Color::Cyan))
            .highlight_symbol("> ");

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        StatefulWidget::render(list, area, buf, state);

        let mut state = self.state.vertical_scroll_state.clone();
        state = state.content_length(playlist.len() as u16);
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }

    type State = ListState;
}

#[derive(Default, Clone)]
struct RoomsWidget {
    state: RoomsWidgetState,
}

#[derive(Default, Clone)]
struct RoomsWidgetState {
    vertical_scroll_state: ScrollbarState,
    user: UserStatus,
    state: ListState,
    style: Style,
    rooms: RoomList,
}

impl RoomsWidget {
    fn new() -> Self {
        RoomsWidget {
            state: Default::default(),
        }
    }

    fn change_style(&mut self, style: Style) {
        self.state.style = style;
    }
    fn next(&mut self) {
        let i = match self.state.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.state.rooms.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.state.selected() {
            Some(i) => {
                if i >= self.state.rooms.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.vertical_scroll_state = self.state.vertical_scroll_state.position(i as u16);
        self.state.state.select(Some(i));
    }

    fn unselect(&mut self) {
        self.state.state.select(None);
    }

    fn get_current_room(&self) -> Option<&RoomName> {
        match self.state.state.selected() {
            Some(index) => self.state.rooms.get_room_name(index),
            None => None,
        }
    }
}

impl StatefulWidget for RoomsWidget {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        //TODO highlight room of own user
        //TODO scrolling
        //TODO change room
        //TODO split listitems since they cannot be display if too large
        let rooms: Vec<ListItem> = self
            .state
            .rooms
            .iter()
            .map(|(room_name, users)| {
                let room_line = Line::from(vec![room_name.gray()]);
                let mut user_lines: Vec<Line> = users
                    .into_iter()
                    .map(|u| {
                        let name = match u.eq(&self.state.user) {
                            true => format!("{} (me)", u.name),
                            false => u.name.clone(),
                        };
                        let mut user_line = match u.ready {
                            true => Line::from(format!("  {} (ready)", name)),
                            false => Line::from(format!("  {} (ready)", name)),
                        };
                        user_line.patch_style(Style::default().fg(Color::Gray));
                        user_line
                    })
                    .collect();
                user_lines.insert(0, room_line);
                ListItem::new(user_lines)
            })
            .collect();

        let messages_block = Block::default()
            .style(self.state.style)
            .title(Title::from("Rooms"))
            .borders(Borders::ALL);

        let rooms_list = List::new(rooms.clone())
            .gray()
            .block(messages_block)
            .highlight_style(Style::default().fg(Color::Cyan));

        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .end_symbol(None);

        StatefulWidget::render(rooms_list, area, buf, state);

        let mut state = self.state.vertical_scroll_state.clone();
        state = state.content_length(rooms.len() as u16);
        scrollbar.render(
            area.inner(&Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state,
        );
    }
}

#[derive(Default, Clone)]
struct InputWidgetState {
    input: String,
    cursor_position: usize,
}

impl InputWidgetState {
    fn cursor_position(&self) -> u16 {
        self.cursor_position as u16
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        let mut chars: Vec<char> = self.input.chars().collect();
        chars.insert(self.cursor_position, new_char);
        self.input = chars.into_iter().collect();
        self.move_cursor_right();
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.cursor_position != 0;
        if is_not_cursor_leftmost {
            let current_index = self.cursor_position;
            let from_left_to_current_index = current_index - 1;
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.cursor_position = 0;
    }

    fn submit_message(&mut self) -> String {
        let msg = self.input.clone();
        self.flush();
        msg
    }

    fn flush(&mut self) {
        self.input.clear();
        self.reset_cursor();
    }
}

#[derive(Default, Clone)]
struct CommandInputWidget {
    state: InputWidgetState,
    active: bool,
}

impl CommandInputWidget {
    fn new() -> Self {
        CommandInputWidget {
            state: Default::default(),
            active: false,
        }
    }

    fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    fn cursor_position(&self) -> u16 {
        self.state.cursor_position()
    }

    fn move_cursor_left(&mut self) {
        self.state.move_cursor_left();
    }

    fn move_cursor_right(&mut self) {
        self.state.move_cursor_right();
    }

    fn enter_char(&mut self, new_char: char) {
        self.state.enter_char(new_char);
    }

    fn delete_char(&mut self) {
        self.state.delete_char();
    }

    fn submit_message(&mut self) -> String {
        self.state.submit_message()
    }

    fn flush(&mut self) {
        self.state.flush();
    }
}

impl Widget for CommandInputWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let input_text = match self.active {
            true => {
                let mut text = ":".to_string();
                text.push_str(&self.state.input.clone());
                text
            }
            false => self.state.input.clone(),
        };
        let input_block = Paragraph::new(input_text).block(Block::default().borders(Borders::NONE));

        input_block.render(area, buf);
    }
}

#[derive(Default, Clone)]
struct ChatInputWidget {
    state: InputWidgetState,
    style: Style,
}

impl ChatInputWidget {
    fn new() -> Self {
        Default::default()
    }

    fn change_style(&mut self, style: Style) {
        self.style = style;
    }

    fn cursor_position(&self) -> u16 {
        self.state.cursor_position()
    }

    fn move_cursor_left(&mut self) {
        self.state.move_cursor_left();
    }

    fn move_cursor_right(&mut self) {
        self.state.move_cursor_right();
    }

    fn enter_char(&mut self, new_char: char) {
        self.state.enter_char(new_char);
    }

    fn delete_char(&mut self) {
        self.state.delete_char();
    }

    fn submit_message(&mut self) -> String {
        self.state.submit_message()
    }

    fn flush(&mut self) {
        self.state.flush();
    }
}

//TODO increase chat size if text exceeds size
//TODO wrap cursor
//TODO wrap all input widgets into tui-textarea
//TODO add padding and titles for blocks
impl Widget for OverlayLoginWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let address_input_text = self.address_field_state.input.clone();
        let password_input_text = self.password_field_state.input.clone();

        let outer_block = Block::default().title("Login").borders(Borders::ALL).gray();

        let text_block = Paragraph::new(Text::raw(
            "Please input the server address and password.\nPress Enter to submit.",
        ))
        .block(Block::default().borders(Borders::NONE).gray())
        .wrap(Wrap { trim: false });

        let input_colors = match self.selected {
            SelectedField::Address => (self.style, Style::default().fg(Color::Gray)),
            SelectedField::Password => (Style::default().fg(Color::Gray), self.style),
        };

        let address_input_block = Paragraph::new(address_input_text)
            .block(
                Block::default()
                    .title("Address")
                    .borders(Borders::ALL)
                    .style(input_colors.0),
            )
            .wrap(Wrap { trim: false });

        let password_input_block = Paragraph::new(password_input_text)
            .block(
                Block::default()
                    .title("Password")
                    .borders(Borders::ALL)
                    .style(input_colors.1),
            )
            .wrap(Wrap { trim: false });

        let layout = Layout::default()
            .constraints(
                [
                    Constraint::Min(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(area);

        outer_block.render(area, buf);
        text_block.render(layout[0], buf);
        address_input_block.render(layout[1], buf);
        password_input_block.render(layout[2], buf);
    }
}

enum LoopControl {
    Continue,
    Break,
}

enum Mode {
    Normal,
    Editing,
    Inspecting,
    Overlay,
}

struct App {
    chat_widget: ChatWidget,
    database_widget: DatabaseWidget,
    rooms_widget: RoomsWidget,
    playlist_widget: PlaylistWidget,
    command_input_widget: CommandInputWidget,
    chat_input_widget: ChatInputWidget,
    login_overlay_widget: OverlayLoginWidget,
    current_widget: WidgetType,
    overlay_widget: Option<OverlayType>,
    mode: Mode,
}

impl App {
    fn new() -> App {
        App {
            chat_widget: {
                let mut chat = ChatWidget::new();
                chat.change_style(Style::default().fg(Color::Magenta));
                chat
            },
            database_widget: DatabaseWidget::new(),
            rooms_widget: RoomsWidget::new(),
            playlist_widget: PlaylistWidget::new(),
            command_input_widget: CommandInputWidget::new(),
            chat_input_widget: ChatInputWidget::new(),
            login_overlay_widget: OverlayLoginWidget::new(),
            current_widget: WidgetType::Chat,
            overlay_widget: None,
            mode: Mode::Normal,
        }
    }

    fn next_state_on_left(&mut self) {
        match self.current_widget {
            WidgetType::Database => self.database_widget.change_style(Style::default()),
            WidgetType::Rooms => self.rooms_widget.change_style(Style::default()),
            WidgetType::Playlist => self.playlist_widget.change_style(Style::default()),
            _ => {}
        }
        self.chat_input_widget
            .change_style(Style::default().fg(Color::Magenta));
        self.current_widget = WidgetType::ChatInput;
    }

    fn next_state_on_right(&mut self) {
        match self.current_widget {
            WidgetType::Chat => {
                self.chat_widget.change_style(Style::default());
                self.database_widget
                    .change_style(Style::default().fg(Color::Magenta));
                self.current_widget = WidgetType::Database;
            }
            WidgetType::ChatInput => {
                self.chat_input_widget.change_style(Style::default());
                self.playlist_widget
                    .change_style(Style::default().fg(Color::Magenta));
                self.current_widget = WidgetType::Playlist;
            }
            _ => {}
        }
    }

    fn next_state_on_up(&mut self) {
        match self.current_widget {
            WidgetType::Playlist => {
                self.playlist_widget.change_style(Style::default());
                self.current_widget = WidgetType::Rooms;
                self.rooms_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            WidgetType::Rooms => {
                self.rooms_widget.change_style(Style::default());
                self.current_widget = WidgetType::Database;
                self.database_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            WidgetType::ChatInput => {
                self.chat_input_widget.change_style(Style::default());
                self.current_widget = WidgetType::Chat;
                self.chat_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            _ => {}
        }
    }

    fn next_state_on_down(&mut self) {
        match self.current_widget {
            WidgetType::Database => {
                self.database_widget.change_style(Style::default());
                self.current_widget = WidgetType::Rooms;
                self.rooms_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            WidgetType::Rooms => {
                self.rooms_widget.change_style(Style::default());
                self.current_widget = WidgetType::Playlist;
                self.playlist_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            WidgetType::Chat => {
                self.chat_widget.change_style(Style::default());
                self.current_widget = WidgetType::ChatInput;
                self.chat_input_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            _ => {}
        }
    }

    fn highlight_selection(&mut self) {
        match self.current_widget {
            WidgetType::Chat => self
                .chat_widget
                .change_style(Style::default().fg(Color::Cyan)),
            WidgetType::Database => {
                self.database_widget
                    .change_style(Style::default().fg(Color::Cyan));
            }
            WidgetType::Rooms => self
                .rooms_widget
                .change_style(Style::default().fg(Color::Cyan)),
            WidgetType::Playlist => {
                self.playlist_widget
                    .change_style(Style::default().fg(Color::Cyan));
                self.playlist_widget.next();
            }
            WidgetType::ChatInput => {
                self.chat_input_widget
                    .change_style(Style::default().fg(Color::Cyan));
            }
            _ => {}
        }
    }

    fn unhighlight_selection(&mut self) {
        match self.current_widget {
            WidgetType::Chat => self
                .chat_widget
                .change_style(Style::default().fg(Color::Magenta)),
            WidgetType::Database => {
                self.database_widget
                    .change_style(Style::default().fg(Color::Magenta));
            }
            WidgetType::Rooms => self
                .rooms_widget
                .change_style(Style::default().fg(Color::Magenta)),
            WidgetType::Playlist => self
                .playlist_widget
                .change_style(Style::default().fg(Color::Magenta)),
            WidgetType::ChatInput => self
                .chat_input_widget
                .change_style(Style::default().fg(Color::Magenta)),
            _ => {}
        }
    }
}

impl RatatuiView {
    pub fn new(model: UiModel) -> Self {
        let terminal = Self::setup_terminal().expect("tui setup failed");
        let app = App::new();
        Self {
            terminal,
            app,
            model,
        }
    }

    pub async fn start(&mut self) {
        let original_hook = std::panic::take_hook();

        std::panic::set_hook(Box::new(move |panic| {
            Self::restore_terminal().expect("restore terminal failed");
            original_hook(panic);
        }));

        self.run().await.expect("app loop failed");
        Self::restore_terminal().expect("restore terminal failed");
        std::process::exit(0);
    }

    fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
        let mut stdout = io::stdout();
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
            .context("unable to enter alternate screen")?;
        Terminal::new(CrosstermBackend::new(stdout)).context("creating terminal failed")
    }

    fn restore_terminal() -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)
            .context("unable to switch to main screen")?;

        Ok(())
    }

    async fn run(&mut self) -> io::Result<()> {
        let mut event = EventStream::new();
        let notify = self.model.notify.clone();

        loop {
            self.terminal.draw(|f| Self::ui(f, &mut self.app))?;

            tokio::select! {
                ct_event = event.next() => {
                    match self.handle_event(ct_event) {
                        LoopControl::Break => break,
                        _ => {}
                    }
                },
                _ = notify.notified() => {
                    self.handle_notify()
                }
            }
        }

        Ok(())
    }

    fn handle_event(&mut self, ct_event: Option<Result<Event, std::io::Error>>) -> LoopControl {
        if let Some(Ok(event)) = ct_event {
            match self.app.mode {
                Mode::Normal => return self.handle_normal_event(&event),
                Mode::Inspecting => self.handle_inspecting_event(&event),
                Mode::Editing => self.handle_editing_event(&event),
                Mode::Overlay => self.handle_overlay_event(&event),
            }
        }

        LoopControl::Continue
    }

    fn handle_editing_event(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.command_input_widget.flush();
                            self.app.command_input_widget.set_active(false);
                        }
                        KeyCode::Enter => {
                            self.app.mode = Mode::Normal;
                            self.app.command_input_widget.set_active(false);
                            let msg = self.app.command_input_widget.submit_message();
                            self.parse_commands(msg);
                        }
                        KeyCode::Char(to_insert) => {
                            self.app.command_input_widget.enter_char(to_insert);
                        }
                        KeyCode::Backspace => {
                            self.app.command_input_widget.delete_char();
                        }
                        KeyCode::Left => {
                            self.app.command_input_widget.move_cursor_left();
                        }
                        KeyCode::Right => {
                            self.app.command_input_widget.move_cursor_right();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    //TODO returning errors
    fn parse_commands(&mut self, msg: String) {
        //TODO refactor
        let args: Vec<&str> = msg.split_whitespace().collect();
        let args = args.as_slice();

        match args {
            ["w", msg @ ..] | ["write", msg @ ..] => self.model.send_message(msg.concat()),
            ["server-change", addr, secure, password, room]
            | ["sc", addr, secure, password, room] => self.handle_server_change(
                addr.to_string(),
                secure,
                Some(password.to_string()),
                room.to_string(),
            ),
            ["server-change", addr, secure, room] | ["sc", addr, secure, room] => {
                self.handle_server_change(addr.to_string(), secure, None, room.to_string())
            }
            ["room-change", room] | ["rc", room] => {
                self.model.change_room(RoomChange::from(room.to_string()))
            }
            ["username-change", username] | ["uc", username] => {
                self.model.change_username(username.to_string())
            }
            ["toggle-ready"] | ["tr"] => self.model.user_ready_toggle(),
            ["start-update"] => self.model.start_db_update(),
            ["stop-update"] => self.model.stop_db_update(),
            ["delete", filename] | ["d", filename] => self.remove(&PlaylistVideo::from(*filename)),

            ["move", filename, position] | ["mv", filename, position] => {
                self.handle_move(filename, position)
            }
            ["add", filename] => self.add(&PlaylistVideo::from(*filename)),
            _ => {}
        }
    }

    fn add(&self, video: &PlaylistVideo) {
        //TODO refactor
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.insert(0, video.clone());
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    fn remove(&self, video: &PlaylistVideo) {
        //TODO refactor
        let mut updated_playlist = self.model.playlist.get_inner();
        if updated_playlist.remove_by_video(video).is_some() {
            self.model.playlist.set(updated_playlist.clone());
            self.model.change_playlist(updated_playlist);
        }
    }

    fn remove_range(&self, positions: (usize, usize)) {
        //TODO refactor
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.remove_range(positions.0..=positions.1);
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    fn move_to(&mut self, filename: &str, position: usize) {
        //TODO refactor
        let mut updated_playlist = self.model.playlist.get_inner();
        if let Some(pos) = updated_playlist.find(&PlaylistVideo::from(filename)) {
            updated_playlist.remove(pos);
            if updated_playlist.len() > position.saturating_sub(1) {
                updated_playlist.insert(position.saturating_sub(1), filename.into());
            } else {
                updated_playlist.append(filename.into());
            }
            self.model.change_playlist(updated_playlist);
        }
    }

    fn handle_server_change(
        &mut self,
        addr: String,
        secure: &str,
        password: Option<String>,
        room: String,
    ) {
        //TODO refactor
        let secure: bool = match secure {
            "true" => true,
            "false" => false,
            _ => return,
        };

        let room = RoomChange { room };

        self.model.change_server(ServerChange {
            addr,
            secure,
            password,
            room,
        });
    }

    fn handle_move(&mut self, filename: &str, position: &str) {
        let position = match position.parse::<usize>() {
            Ok(value) => value,
            _ => return,
        };

        self.move_to(filename, position);
    }

    fn handle_inspecting_event(&mut self, event: &Event) {
        match self.app.current_widget {
            WidgetType::Playlist => self.handle_inspecting_event_playlist(event),
            WidgetType::Chat => self.handle_inspecting_event_chat(event),
            WidgetType::Rooms => self.handle_inspecting_event_rooms(event),
            WidgetType::Database => self.handle_inspecting_event_database(event),
            WidgetType::ChatInput => self.handle_inspecting_event_chat_input(event),
            _ => {}
        }
    }

    fn handle_inspecting_event_database(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.unhighlight_selection();
                        }
                        KeyCode::Enter => self.model.start_db_update(),
                        KeyCode::Backspace => self.model.stop_db_update(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_inspecting_event_rooms(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.unhighlight_selection();
                            self.app.playlist_widget.unselect();
                        }
                        KeyCode::Up => {
                            self.app.rooms_widget.next();
                        }
                        KeyCode::Down => {
                            self.app.rooms_widget.previous();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_inspecting_event_chat(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.unhighlight_selection();
                        }
                        KeyCode::Down => self.app.chat_widget.previous(),
                        KeyCode::Up => self.app.chat_widget.next(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_inspecting_event_playlist(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.unhighlight_selection();
                            self.app.playlist_widget.unselect();
                        }
                        KeyCode::Enter => match self.app.playlist_widget.get_current_video() {
                            Some(video) => self.model.change_video(video.clone()),
                            None => {}
                        },
                        KeyCode::Up => {
                            self.app.playlist_widget.next();
                        }
                        KeyCode::Down => {
                            self.app.playlist_widget.previous();
                        }
                        KeyCode::Char('d') => {
                            match self.app.playlist_widget.get_selected_videos() {
                                Some(index) => {
                                    self.remove_range(index);
                                    self.app.playlist_widget.reset_offset();
                                }
                                None => {}
                            }
                        }
                        KeyCode::Char('x') => {
                            self.app.playlist_widget.increase_selection_offset();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_inspecting_event_chat_input(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.chat_input_widget.flush();
                            self.app
                                .chat_input_widget
                                .change_style(Style::default().fg(Color::Magenta));
                        }
                        KeyCode::Enter => {
                            let msg = self.app.chat_input_widget.submit_message();
                            self.model.send_message(msg);
                        }
                        KeyCode::Char(to_insert) => {
                            self.app.chat_input_widget.enter_char(to_insert);
                        }
                        KeyCode::Backspace => {
                            self.app.chat_input_widget.delete_char();
                        }
                        KeyCode::Left => {
                            self.app.chat_input_widget.move_cursor_left();
                        }
                        KeyCode::Right => {
                            self.app.chat_input_widget.move_cursor_right();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_normal_event(&mut self, event: &Event) -> LoopControl {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return LoopControl::Break,
                        KeyCode::Char(':') => {
                            self.app.mode = Mode::Editing;
                            self.app.command_input_widget.set_active(true);
                        }
                        KeyCode::Enter => {
                            self.app.mode = Mode::Inspecting;
                            self.app.highlight_selection();
                        }
                        KeyCode::Char(' ') => {
                            self.app.mode = Mode::Overlay;
                            self.app.overlay_widget = Some(OverlayType::Option);
                        }
                        KeyCode::Right => self.app.next_state_on_right(),
                        KeyCode::Left => self.app.next_state_on_left(),
                        KeyCode::Down => self.app.next_state_on_down(),
                        KeyCode::Up => self.app.next_state_on_up(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        LoopControl::Continue
    }

    fn handle_overlay_event(&mut self, event: &Event) {
        match &self.app.overlay_widget {
            Some(overlay) => match overlay {
                OverlayType::Option => self.handle_overlay_event_option(event),
                OverlayType::Help => todo!(),
                OverlayType::Login => self.handle_overlay_event_login(event),
            },
            None => self.app.mode = Mode::Normal,
        }
    }

    fn handle_overlay_event_option(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('h') => self.app.overlay_widget = Some(OverlayType::Help),
                        KeyCode::Char('l') => self.app.overlay_widget = Some(OverlayType::Login),
                        _ => {
                            self.app.mode = Mode::Normal;
                            self.app.overlay_widget = None;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_overlay_event_login(&mut self, event: &Event) {
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => {
                            self.app.mode = Mode::Normal;
                            self.app.overlay_widget = None;
                        }
                        KeyCode::Left => self.app.login_overlay_widget.move_cursor_left(),
                        KeyCode::Right => self.app.login_overlay_widget.move_cursor_right(),
                        KeyCode::Up => self.app.login_overlay_widget.change_selected(),
                        KeyCode::Down => self.app.login_overlay_widget.change_selected(),
                        KeyCode::Char(input) => self.app.login_overlay_widget.enter_char(input),
                        KeyCode::Enter => {
                            self.app.mode = Mode::Normal;
                            let msg = self.app.login_overlay_widget.submit_message();
                            self.model.change_server(ServerChange {
                                addr: msg.0,
                                secure: false,
                                password: Some(msg.1),
                                room: RoomChange {
                                    room: "".to_string(),
                                },
                            });
                            self.app.login_overlay_widget.reset();
                        }
                        KeyCode::Backspace => self.app.login_overlay_widget.delete_char(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_notify(&mut self) {
        //TODO setters in widgets
        if self.model.file_database_status.changed() {
            let file_db_status = self.model.file_database_status.get_inner();
            self.app.database_widget.state.file_database_status = (file_db_status * 100.0) as u16;
        }

        if self.model.file_database.changed() {
            let file_db = self.model.file_database.get_inner();
            self.app.database_widget.state.file_database = file_db;
        }

        if self.model.playlist.changed() {
            let playlist = self.model.playlist.get_inner();
            self.app.playlist_widget.state.playlist = playlist.clone();
        }

        let messages = self.model.messages.get_inner().iter().cloned().collect();
        self.app.chat_widget.state.messages = messages;
        self.app.chat_widget.update_cursor_latest();
        if self.model.messages.changed() {}

        if self.model.room_list.changed() {
            let rooms = self.model.room_list.get_inner();
            self.app.rooms_widget.state.rooms = rooms;
        }

        if self.model.user.changed() {
            let user = self.model.user.get_inner();
            self.app.rooms_widget.state.user = user.clone();
            self.app.chat_widget.state.user = user;
        }
    }

    fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App) {
        Self::render_player(f, app);
    }

    fn render_player<B: Backend>(f: &mut Frame<B>, app: &mut App) {
        let size = f.size();
        let main_vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(size);

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(main_vertical_chunks[0]);

        let vertical_left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(4)].as_ref())
            .split(horizontal_chunks[0]);

        let vertical_right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Percentage(20),
                    Constraint::Min(0),
                ]
                .as_ref(),
            )
            .split(horizontal_chunks[1]);

        f.render_widget(app.database_widget.clone(), vertical_right_chunks[0]);
        f.render_stateful_widget(
            app.rooms_widget.clone(),
            vertical_right_chunks[1],
            &mut app.rooms_widget.state.state,
        );
        f.render_stateful_widget(
            app.playlist_widget.clone(),
            vertical_right_chunks[2],
            &mut app.playlist_widget.state.state,
        );
        f.render_stateful_widget(
            app.chat_widget.clone(),
            vertical_left_chunks[0],
            &mut app.chat_widget.state.state,
        );
        f.render_widget(app.chat_input_widget.clone(), vertical_left_chunks[1]);
        f.render_widget(app.command_input_widget.clone(), main_vertical_chunks[1]);

        match app.mode {
            Mode::Overlay => match &app.overlay_widget {
                Some(overlay) => match overlay {
                    OverlayType::Option => {
                        let options_widget = OverlayOptionsWidget::new();
                        let area = options_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(options_widget, area);
                    }
                    OverlayType::Login => {
                        let area = app.login_overlay_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(app.login_overlay_widget.clone(), area);
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
        //TODO wrap cursor into new line as well
        match app.mode {
            Mode::Editing => {
                f.set_cursor(
                    main_vertical_chunks[1].x
                        + app.command_input_widget.cursor_position().saturating_add(1),
                    main_vertical_chunks[1].y.saturating_add(1),
                );
            }
            Mode::Inspecting => match app.current_widget {
                WidgetType::ChatInput => {
                    f.set_cursor(
                        vertical_left_chunks[1].x
                            + app.chat_input_widget.cursor_position().saturating_add(1),
                        vertical_left_chunks[1].y.saturating_add(1),
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }
}
