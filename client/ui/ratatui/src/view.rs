use std::io::{self, Stdout};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::future::OptionFuture;
use futures::{Future, StreamExt};
use niketsu_core::config::Config;
use niketsu_core::file_database::fuzzy::FuzzySearch;
use niketsu_core::playlist::Video;
use niketsu_core::ui::{RoomChange, ServerChange, UiModel, UserInterface};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::widget::login::LoginWidget;
use super::widget::playlist::PlaylistWidget;
use crate::handler::chat::Chat;
use crate::handler::command::Command;
use crate::handler::options::Options;
use crate::handler::{EventHandler, MainEventHandler, OverlayState, State};
use crate::widget::chat::{ChatWidget, ChatWidgetState};
use crate::widget::chat_input::{ChatInputWidget, ChatInputWidgetState};
use crate::widget::command::{CommandInputWidget, CommandInputWidgetState};
use crate::widget::database::{DatabaseWidget, DatabaseWidgetState};
use crate::widget::fuzzy_search::{FuzzySearchWidget, FuzzySearchWidgetState};
use crate::widget::help::{HelpWidget, HelpWidgetState};
use crate::widget::login::LoginWidgetState;
use crate::widget::media::{MediaDirWidget, MediaDirWidgetState};
use crate::widget::options::{OptionsWidget, OptionsWidgetState};
use crate::widget::playlist::PlaylistWidgetState;
use crate::widget::room::{RoomsWidget, RoomsWidgetState};
use crate::widget::OverlayWidgetState;

pub struct RatatuiView {
    pub app: App,
    pub model: UiModel,
    pub config: Config,
}

enum LoopControl {
    Continue,
    Break,
}

#[derive(Debug, Default, Clone)]
pub enum Mode {
    #[default]
    Normal,
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
    pub command_input_widget: CommandInputWidgetState,
    pub chat_input_widget: ChatInputWidgetState,
    pub current_overlay_state: Option<OverlayState>,
    pub options_widget_state: OptionsWidgetState,
    pub help_widget_state: HelpWidgetState,
    pub login_widget_state: LoginWidgetState,
    pub media_widget_state: MediaDirWidgetState,
    pub fuzzy_search_widget_state: FuzzySearchWidgetState,
    pub current_search: Option<FuzzySearch>,
    mode: Mode,
    prev_mode: Option<Mode>,
}

impl App {
    fn new(config: Config) -> App {
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
            command_input_widget: CommandInputWidgetState::default(),
            chat_input_widget: ChatInputWidgetState::new(),
            current_overlay_state: None,
            options_widget_state: OptionsWidgetState::default(),
            help_widget_state: HelpWidgetState::new(),
            login_widget_state: LoginWidgetState::new(&config),
            fuzzy_search_widget_state: FuzzySearchWidgetState::new(),
            media_widget_state: MediaDirWidgetState::new(config.media_dirs),
            current_search: None,
            mode: Mode::Normal,
            prev_mode: None,
        }
    }

    pub fn set_current_state(&mut self, state: State) {
        self.current_state = state;
    }

    pub fn set_current_overlay_state(&mut self, state: Option<OverlayState>) {
        self.current_overlay_state = state;
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.prev_mode = Some(self.mode.clone());
        self.mode = mode;
    }

    pub fn reset_overlay(&mut self) {
        self.current_overlay_state = None;
        if let Some(mode) = self.prev_mode.clone() {
            self.mode = mode;
        } else {
            self.mode = Mode::Normal;
        }
    }

    pub fn current_state(&self) -> State {
        self.current_state
    }

    pub fn fuzzy_search(&mut self, query: String) {
        self.current_search = Some(self.fuzzy_search_widget_state.fuzzy_search(query));
    }

    pub fn reset_fuzzy_search(&mut self) {
        self.current_search = None;
    }
}

impl RatatuiView {
    pub fn create(
        config: Config,
    ) -> (
        UserInterface,
        Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
    ) {
        let ui = UserInterface::new(&config);
        let app = App::new(config.clone());
        let mut view = Self {
            app,
            model: ui.model().clone(),
            config,
        };
        let handle = Box::pin(async move { view.run().await });
        (ui, handle)
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = Self::setup_terminal().expect("tui setup failed");
        let mut event = EventStream::new();
        let mut tick_rate = tokio::time::interval(Duration::from_millis(50));
        let notify = self.model.notify.clone();

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            Self::restore_terminal().expect("restore terminal failed");
            original_hook(panic);
        }));

        let mut needs_update = false;
        terminal.draw(|f| Self::render(f, &mut self.app))?;
        loop {
            tokio::select! {
                ct_event = event.next() => {
                    if let LoopControl::Break = self.handle_event(ct_event) {
                        break;
                    }
                    needs_update = true;
                },
                Some(search_result) = OptionFuture::from(self.app.current_search.as_mut()) => {
                    self.app.fuzzy_search_widget_state.set_result(search_result);
                    self.app.current_search = None;
                    needs_update = true;
                },
                _ = notify.notified() => {
                    self.handle_notify();
                        needs_update = true;
                },
                _ = tick_rate.tick() => {
                    if needs_update {
                        terminal.draw(|f| Self::render(f, &mut self.app))?;
                        needs_update = false;
                    }
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
                Mode::Inspecting => self
                    .app
                    .current_state
                    .clone()
                    .handle_with_overlay(self, &event),
                Mode::Overlay => {
                    if let Some(overlay) = self.app.current_overlay_state {
                        overlay.handle(self, &event)
                    }
                }
            }
        }

        LoopControl::Continue
    }

    fn handle_normal_event(&mut self, event: &Event) -> LoopControl {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => return LoopControl::Break,
                    KeyCode::Char(':') => {
                        self.app.set_mode(Mode::Overlay);
                        self.app
                            .set_current_overlay_state(Some(OverlayState::from(Command {})));
                        self.app.command_input_widget.set_active(true);
                    }
                    KeyCode::Enter => {
                        self.app.mode = Mode::Inspecting;
                        self.highlight();
                    }
                    KeyCode::Char(' ') => {
                        self.app.set_mode(Mode::Overlay);
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
            self.app
                .fuzzy_search_widget_state
                .set_file_database(file_db);
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

    fn render(f: &mut Frame, app: &mut App) {
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

        f.render_stateful_widget(
            ChatInputWidget {},
            vertical_left_chunks[1],
            &mut app.chat_input_widget,
        );

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
                    OverlayState::Help(_help) => {
                        let area = app.help_widget_state.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(HelpWidget {}, area, &mut app.help_widget_state);
                    }
                    OverlayState::Login(_login) => {
                        let area = app.login_widget_state.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(LoginWidget {}, area, &mut app.login_widget_state);
                    }
                    OverlayState::FuzzySearch(_fuzzy_search) => {
                        let area = app.fuzzy_search_widget_state.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(
                            FuzzySearchWidget {},
                            area,
                            &mut app.fuzzy_search_widget_state,
                        );
                    }
                    OverlayState::MediaDir(_media_dir) => {
                        let area = app.media_widget_state.area(size);
                        f.render_widget(Clear, area);
                        f.render_stateful_widget(
                            MediaDirWidget {},
                            area,
                            &mut app.media_widget_state,
                        );
                    }
                    OverlayState::Command(_command) => {
                        f.render_stateful_widget(
                            CommandInputWidget {},
                            main_vertical_chunks[1],
                            &mut app.command_input_widget,
                        );
                    }
                }
            }
        }
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
            ["add", filename] => self.insert(0, &Video::from(*filename)),
            _ => {}
        }
    }

    pub fn insert(&self, index: usize, video: &Video) {
        let mut updated_playlist = self.model.playlist.get_inner();
        let saturated_index = std::cmp::min(index, updated_playlist.len());
        updated_playlist.insert(saturated_index, video.clone());
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

    pub fn change_media_dirs(&mut self, paths: Vec<PathBuf>) {
        self.model.change_db_paths(paths)
    }

    pub fn save_config(
        &mut self,
        address: String,
        secure: bool,
        password: String,
        room: String,
        username: String,
    ) {
        self.config.url = address;
        self.config.secure = secure;
        self.config.password = password;
        self.config.room = room;
        self.config.username = username;
        _ = self.config.save();
    }

    pub fn save_media_dir(&mut self, paths: Vec<String>) {
        self.config.media_dirs = paths;
        _ = self.config.save();
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
}
