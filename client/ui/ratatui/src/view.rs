use std::io::{self, Stdout};

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
use niketsu_core::config::Config;
use niketsu_core::file_database::fuzzy::FuzzySearch;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{RoomChange, ServerChange, UiModel, UserInterface};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::widget::login::LoginWidget;
use super::widget::playlist::PlaylistWidget;
use crate::handler::chat::Chat;
use crate::handler::command::handle_command_prompt;
use crate::handler::options::Options;
use crate::handler::{EventHandler, MainEventHandler, OverlayState, State};
use crate::widget::chat::{ChatWidget, ChatWidgetState};
use crate::widget::chat_input::ChatInputWidget;
use crate::widget::command::CommandInputWidget;
use crate::widget::database::{DatabaseWidget, DatabaseWidgetState};
use crate::widget::fuzzy_search::FuzzySearchWidget;
use crate::widget::options::{OptionsWidget, OptionsWidgetState};
use crate::widget::playlist::PlaylistWidgetState;
use crate::widget::room::{RoomsWidget, RoomsWidgetState};
use crate::widget::OverlayWidgetState;

pub struct RatatuiView {
    pub app: App,
    pub model: UiModel,
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
pub struct App {
    current_state: State,
    pub chat_widget_state: ChatWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub rooms_widget_state: RoomsWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub command_input_widget: CommandInputWidget,
    pub chat_input_widget: ChatInputWidget,
    pub current_overlay_state: Option<OverlayState>,
    pub options_widget_state: OptionsWidgetState,
    //TODO pub help_widget: HelpWidget,
    pub login_widget: LoginWidget,
    pub fuzzy_search_widget: FuzzySearchWidget,
    pub current_search: Option<FuzzySearch>,
    mode: Mode,
}

impl App {
    fn new() -> App {
        App {
            current_state: State::from(Chat {}),
            chat_widget_state: {
                let mut chat = ChatWidgetState::default();
                chat.set_style(Style::default().fg(Color::Magenta));
                chat
            },
            database_widget_state: DatabaseWidgetState::default(),
            rooms_widget_state: RoomsWidgetState::default(),
            playlist_widget_state: PlaylistWidgetState::default(),
            command_input_widget: CommandInputWidget::default(),
            chat_input_widget: ChatInputWidget::new(),
            current_overlay_state: None,
            options_widget_state: OptionsWidgetState::new(),
            login_widget: LoginWidget::new(),
            fuzzy_search_widget: FuzzySearchWidget::new(),
            current_search: None,
            mode: Mode::Normal,
        }
    }

    pub fn set_current_state(&mut self, state: State) {
        self.current_state = state;
    }

    pub fn set_current_overlay_state(&mut self, state: Option<OverlayState>) {
        self.current_overlay_state = state;
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn reset_overlay(&mut self) {
        self.current_overlay_state = None;
        self.mode = Mode::Normal;
    }

    pub fn fuzzy_search(&mut self, query: String) {
        self.current_search = Some(self.fuzzy_search_widget.fuzzy_search(query));
    }
}

impl RatatuiView {
    pub fn create(config: Config) -> (UserInterface, Box<dyn FnOnce() -> anyhow::Result<()>>) {
        let ui = UserInterface::default();
        let app = App::new();
        let mut view = Self {
            app,
            model: ui.model().clone(),
        };
        let handle = Box::new(move || futures::executor::block_on(view.run()));
        (ui, handle)
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
                Mode::Inspecting => self.app.current_state.clone().handle(self, &event),
                Mode::Editing => handle_command_prompt(self, &event),
                Mode::Overlay => {
                    if let Some(overlay) = self.app.current_overlay_state {
                        overlay.handle(self, &event)
                    }
                }
            }
        }

        LoopControl::Continue
    }

    // TODO move to app
    pub fn transition(&mut self, to: State) {
        self.app
            .current_state
            .clone()
            .set_style(self, Style::default().fg(Color::default()));
        self.app.current_state = to;
        self.app
            .current_state
            .clone()
            .set_style(self, Style::default().fg(Color::Magenta));
    }

    pub fn highlight(&mut self) {
        self.app
            .current_state
            .clone()
            .set_style(self, Style::default().fg(Color::Cyan));
    }

    pub fn hover_highlight(&mut self) {
        self.app
            .current_state
            .clone()
            .set_style(self, Style::default().fg(Color::Magenta));
    }

    //TODO returning errors
    pub fn parse_commands(&mut self, msg: String) {
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
            ["delete", filename] | ["d", filename] => self.remove(&Video::from(*filename)),

            ["move", filename, position] | ["mv", filename, position] => {
                self.handle_move(filename, position)
            }
            ["add", filename] => self.add(&Video::from(*filename)),
            _ => {}
        }
    }

    pub fn add(&self, video: &Video) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.insert(0, video.clone());
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    fn remove(&self, video: &Video) {
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

    pub fn append_at(&self, index: usize, videos: Vec<Video>) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.append_at(index, videos.into_iter());
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    fn move_to(&mut self, video: &Video, index: usize) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.move_video(video, index);
        self.model.change_playlist(updated_playlist);
    }

    pub fn select(&mut self, video: Video) {
        self.model.change_video(video)
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

        self.move_to(&Video::from(filename), position);
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
                        self.highlight();
                    }
                    KeyCode::Char(' ') => {
                        self.app.mode = Mode::Overlay;
                        self.app
                            .set_current_overlay_state(Some(OverlayState::from(Options {})));
                    }
                    KeyCode::Right | KeyCode::Left | KeyCode::Down | KeyCode::Up => {
                        self.app.current_state.clone().handle_next(self, key)
                    }
                    _ => {}
                }
            }
        }

        LoopControl::Continue
    }

    fn handle_notify(&mut self) {
        if self.model.file_database_status.changed() {
            let file_db_status = self.model.file_database_status.get_inner();
            self.app
                .database_widget_state
                .set_file_database_status((file_db_status * 100.0) as u16);
        }

        if self.model.file_database.changed() {
            let file_db = self.model.file_database.get_inner();
            self.app
                .database_widget_state
                .set_file_database(file_db.clone());
            self.app.fuzzy_search_widget.set_file_database(file_db);
        }

        if self.model.playlist.changed() {
            let playlist = self.model.playlist.get_inner();
            self.app.playlist_widget_state.set_playlist(playlist);
        }

        if self.model.messages.changed() {
            let messages = self.model.messages.get_inner().iter().cloned().collect();
            self.app.chat_widget_state.set_messages(messages);
            self.app.chat_widget_state.update_cursor_latest();
        }

        if self.model.room_list.changed() {
            let rooms = self.model.room_list.get_inner();
            self.app.rooms_widget_state.set_rooms(rooms);
        }

        if self.model.user.changed() {
            let user = self.model.user.get_inner();
            self.app.rooms_widget_state.set_user(user.clone());
            self.app.chat_widget_state.set_user(user);
        }

        if self.model.playing_video.changed() {
            let playing_video = self.model.playing_video.get_inner();
            self.app
                .playlist_widget_state
                .set_playing_video(playing_video);
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

        f.render_stateful_widget(
            DatabaseWidget,
            vertical_right_chunks[0],
            &mut app.database_widget_state,
        );
        f.render_stateful_widget(
            RoomsWidget,
            vertical_right_chunks[1],
            &mut app.rooms_widget_state,
        );
        f.render_stateful_widget(
            PlaylistWidget,
            vertical_right_chunks[2],
            &mut app.playlist_widget_state,
        );
        f.render_stateful_widget(
            ChatWidget {},
            vertical_left_chunks[0],
            &mut app.chat_widget_state,
        );
        f.render_widget(app.chat_input_widget.clone(), vertical_left_chunks[1]);
        f.render_widget(app.command_input_widget.clone(), main_vertical_chunks[1]);

        if let Mode::Overlay = app.mode {
            if let Some(overlay) = &app.current_overlay_state {
                match overlay {
                    OverlayState::Option(_options) => {
                        let area = app.options_widget_state.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(
                            OptionsWidget {},
                            area,
                            &mut app.options_widget_state,
                        );
                    }
                    OverlayState::Login(_login) => {
                        let area = app.login_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_widget(app.login_widget.clone(), area);
                    }
                    OverlayState::FuzzySearch(_fuzzy_search) => {
                        let area = app.fuzzy_search_widget.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(
                            app.fuzzy_search_widget.clone(),
                            area,
                            &mut app.fuzzy_search_widget.get_state(),
                        );
                    }
                }
            }
        }
    }
}
