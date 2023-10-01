use std::io::{self, Stdout};
use std::ops::Range;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::future::OptionFuture;
use futures::StreamExt;
use niketsu_core::file_database::fuzzy::FuzzySearch;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::PlaylistVideo;
use niketsu_core::rooms::RoomList;
use niketsu_core::ui::{PlayerMessage, RoomChange, ServerChange, UiModel};
use niketsu_core::user::UserStatus;
use ratatui::prelude::*;
use ratatui::symbols::scrollbar;
use ratatui::widgets::block::Title;
use ratatui::widgets::*;

use super::widget::login::LoginWidget;
use super::widget::playlist::PlaylistWidget;
use crate::widget::fuzzy_search::handle::handle_fuzzy_search;
use crate::widget::fuzzy_search::FuzzySearchWidget;
use crate::widget::login::handle::handle_login;
use crate::widget::options::OptionsWidget;
use crate::widget::playlist::handle::handle_playlist;
use crate::widget::OverlayWidget;

pub struct RatatuiView<'a> {
    pub app: App<'a>,
    pub model: UiModel,
}

#[derive(Debug, Default)]
enum WidgetType {
    #[default]
    Chat,
    ChatInput,
    Database,
    Rooms,
    Playlist,
}

#[derive(Debug, Default)]
pub enum OverlayType {
    #[default]
    Login,
    FuzzySearch,
    Option,
    Help,
}

//TODO highlight own user messages
#[derive(Debug, Default, Clone)]
struct ChatWidget {
    state: ChatWidgetState,
}

#[derive(Debug, Default, Clone)]
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
                let head_line = match &t.source {
                    UserMessage(user_name) => {
                        let name = match self.state.user.eq(user_name) {
                            true => {
                                Span::styled(user_name, Style::default().italic().fg(Color::Green))
                            }
                            false => Span::raw(user_name),
                        };
                        Line::from(vec![name, Span::raw(head_message)])
                    }
                    UserAction(user_name) => {
                        let message = format!("User action of {user_name}{head_message}");
                        Line::styled(message, Style::default().fg(Color::LightMagenta))
                    }
                    Server => {
                        let message = format!("Server notification{head_message}");
                        Line::styled(message, Style::default().fg(Color::LightRed))
                    }
                    Internal => {
                        let message = format!("Internal notification{head_message}");
                        Line::styled(message, Style::default().fg(Color::Red))
                    }
                };
                let tail_message = textwrap::fill(t.message.as_str(), area.width as usize);
                let mut message_lines: Vec<Line> = tail_message
                    .split('\n')
                    .map(|l| Line::from(l.to_string()))
                    .collect();
                message_lines.insert(0, head_line);
                ListItem::new(message_lines)
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

        let mut state = self.state.vertical_scroll_state;
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

#[derive(Debug, Default, Clone)]
struct DatabaseWidget {
    state: DatabaseWidgetState,
}

#[derive(Debug, Default, Clone)]
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

//TODO roomswitch
#[derive(Debug, Default, Clone)]
struct RoomsWidget {
    state: RoomsWidgetState,
}

#[derive(Debug, Default, Clone)]
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

    // fn unselect(&mut self) {
    //     self.state.state.select(None);
    // }

    // fn get_current_room(&self) -> Option<&RoomName> {
    //     match self.state.state.selected() {
    //         Some(index) => self.state.rooms.get_room_name(index),
    //         None => None,
    //     }
    // }
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

        let mut state = self.state.vertical_scroll_state;
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

#[derive(Debug, Default, Clone)]
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

#[derive(Debug, Default, Clone)]
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

#[derive(Debug, Default, Clone)]
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

enum LoopControl {
    Continue,
    Break,
}

#[derive(Debug, Default, Clone)]
pub enum Mode {
    #[default]
    Normal,
    Editing,
    Inspecting,
    Overlay,
}

#[derive(Debug, Default)]
pub struct App<'a> {
    current_widget: WidgetType,
    chat_widget: ChatWidget,
    database_widget: DatabaseWidget,
    rooms_widget: RoomsWidget,
    pub playlist_widget: PlaylistWidget,
    command_input_widget: CommandInputWidget,
    chat_input_widget: ChatInputWidget,
    pub overlay_widget: Option<OverlayType>,
    pub options_widget: OptionsWidget,
    // pub help_widget: HelpWidget,
    pub login_widget: LoginWidget<'a>,
    pub fuzzy_search_widget: FuzzySearchWidget<'a>,
    pub current_search: Option<FuzzySearch>,
    mode: Mode,
}

impl<'a> App<'a> {
    fn new() -> App<'a> {
        App {
            current_widget: WidgetType::Chat,
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
            overlay_widget: None,
            options_widget: OptionsWidget::new(),
            login_widget: LoginWidget::new(),
            fuzzy_search_widget: FuzzySearchWidget::new(),
            current_search: None,
            mode: Mode::Normal,
        }
    }

    fn next_state_on_left(&mut self) {
        match self.current_widget {
            WidgetType::Database => self.database_widget.change_style(Style::default()),
            WidgetType::Rooms => self.rooms_widget.change_style(Style::default()),
            WidgetType::Playlist => self.playlist_widget.set_style(Style::default()),
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
                    .set_style(Style::default().fg(Color::Magenta));
                self.current_widget = WidgetType::Playlist;
            }
            _ => {}
        }
    }

    fn next_state_on_up(&mut self) {
        match self.current_widget {
            WidgetType::Playlist => {
                self.playlist_widget.set_style(Style::default());
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
                    .set_style(Style::default().fg(Color::Magenta));
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

    pub fn highlight_selection(&mut self) {
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
                    .set_style(Style::default().fg(Color::Cyan));
                self.playlist_widget.next();
            }
            WidgetType::ChatInput => {
                self.chat_input_widget
                    .change_style(Style::default().fg(Color::Cyan));
            }
        }
    }

    pub fn unhighlight_selection(&mut self) {
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
                .set_style(Style::default().fg(Color::Magenta)),
            WidgetType::ChatInput => self
                .chat_input_widget
                .change_style(Style::default().fg(Color::Magenta)),
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn reset_overlay(&mut self) {
        self.overlay_widget = None;
        self.mode = Mode::Normal;
    }

    pub fn search(&mut self, query: String, file_database: FileStore) {
        self.current_search = Some(FuzzySearch::new(query, file_database));
    }
}

impl<'a> RatatuiView<'a> {
    pub fn new(model: UiModel) -> Self {
        let app = App::new();
        Self { app, model }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = Self::setup_terminal().expect("tui setup failed");
        let mut event = EventStream::new();
        let notify = self.model.notify.clone();

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            Self::restore_terminal().expect("restore terminal failed");
            original_hook(panic);
        }));

        loop {
            terminal.draw(|f| Self::render_player(f, &mut self.app))?;

            tokio::select! {
                ct_event = event.next() => {
                    if let LoopControl::Break = self.handle_event(ct_event) {
                        break;
                    }
                },
                Some(search_result) = OptionFuture::from(self.app.current_search.as_mut()) => {
                    self.app.fuzzy_search_widget.set_result(search_result);
                    self.app.current_search = None;
                },
                _ = notify.notified() => {
                    self.handle_notify()
                }
            }
        }

        Self::restore_terminal()
    }

    fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)
            .context("unable to enter alternate screen")?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
            .context("failed to create terminal")?;
        terminal.hide_cursor().context("failed to hide cursor")?;
        Ok(terminal)
    }

    fn restore_terminal() -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
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
        if let Event::Key(key) = event {
            match key.kind == KeyEventKind::Press {
                true => match key.code {
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
                },
                false => (),
            }
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

    pub fn remove_range(&self, positions: (usize, usize)) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.remove_range(positions.0..=positions.1);
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    pub fn append_at(&self, index: usize, videos: Vec<PlaylistVideo>) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.append_at(index, videos.into_iter());
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    pub fn move_range(&self, range: Range<usize>, index: usize) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.move_range(range, index);
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    fn move_to(&mut self, video: &PlaylistVideo, index: usize) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.move_video(video, index);
        self.model.change_playlist(updated_playlist);
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

        self.move_to(&PlaylistVideo::from(filename), position);
    }

    //TODO refactor
    fn handle_inspecting_event(&mut self, event: &Event) {
        match self.app.current_widget {
            WidgetType::Playlist => handle_playlist(self, event),
            WidgetType::Chat => self.handle_inspecting_event_chat(event),
            WidgetType::Rooms => self.handle_inspecting_event_rooms(event),
            WidgetType::Database => self.handle_inspecting_event_database(event),
            WidgetType::ChatInput => self.handle_inspecting_event_chat_input(event),
        }
    }

    fn handle_inspecting_event_database(&mut self, event: &Event) {
        if let Event::Key(key) = event {
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
    }

    fn handle_inspecting_event_rooms(&mut self, event: &Event) {
        if let Event::Key(key) = event {
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
    }

    fn handle_inspecting_event_chat(&mut self, event: &Event) {
        if let Event::Key(key) = event {
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
    }

    fn handle_inspecting_event_chat_input(&mut self, event: &Event) {
        if let Event::Key(key) = event {
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
    }

    fn handle_normal_event(&mut self, event: &Event) -> LoopControl {
        if let Event::Key(key) = event {
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

        LoopControl::Continue
    }

    fn handle_overlay_event(&mut self, event: &Event) {
        match &self.app.overlay_widget {
            Some(overlay) => match overlay {
                OverlayType::Option => self.handle_overlay_event_option(event),
                OverlayType::Help => todo!(),
                OverlayType::Login => handle_login(self, event),
                OverlayType::FuzzySearch => handle_fuzzy_search(self, event),
            },
            None => self.app.mode = Mode::Normal,
        }
    }

    fn handle_overlay_event_option(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            match key.kind == KeyEventKind::Press {
                true => match key.code {
                    KeyCode::Char('h') => self.app.overlay_widget = Some(OverlayType::Help),
                    KeyCode::Char('l') => self.app.overlay_widget = Some(OverlayType::Login),
                    KeyCode::Char('f') => self.app.overlay_widget = Some(OverlayType::FuzzySearch),
                    _ => {
                        self.app.mode = Mode::Normal;
                        self.app.overlay_widget = None;
                    }
                },
                false => (),
            }
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
            self.app.database_widget.state.file_database = file_db.clone();
            self.app.fuzzy_search_widget.set_file_database(file_db);
        }

        if self.model.playlist.changed() {
            let playlist = self.model.playlist.get_inner();
            self.app.playlist_widget.set_playlist(playlist.clone());
        }

        if self.model.messages.changed() {
            let messages = self.model.messages.get_inner().iter().cloned().collect();
            self.app.chat_widget.state.messages = messages;
            self.app.chat_widget.update_cursor_latest();
        }

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
            &mut app.playlist_widget.state(),
        );
        f.render_stateful_widget(
            app.chat_widget.clone(),
            vertical_left_chunks[0],
            &mut app.chat_widget.state.state,
        );
        f.render_widget(app.chat_input_widget.clone(), vertical_left_chunks[1]);
        f.render_widget(app.command_input_widget.clone(), main_vertical_chunks[1]);

        if let Mode::Overlay = app.mode {
            if let Some(overlay) = &app.overlay_widget {
                match overlay {
                    OverlayType::Option => {
                        let options_widget = OptionsWidget::new();
                        let area = options_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(options_widget, area);
                    }
                    OverlayType::Login => {
                        let area = app.login_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(app.login_widget.clone(), area);
                    }
                    OverlayType::FuzzySearch => {
                        let area = app.fuzzy_search_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(app.fuzzy_search_widget.clone(), area);
                    }
                    _ => {}
                }
            }
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
            Mode::Inspecting => {
                if let WidgetType::ChatInput = app.current_widget {
                    f.set_cursor(
                        vertical_left_chunks[1].x
                            + app.chat_input_widget.cursor_position().saturating_add(1),
                        vertical_left_chunks[1].y.saturating_add(1),
                    );
                }
            }
            _ => {}
        }
    }
}
