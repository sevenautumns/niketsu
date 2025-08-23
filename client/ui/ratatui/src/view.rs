use std::io::{self, Stdout};
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use arboard::Clipboard;
use arcstr::ArcStr;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::future::OptionFuture;
use futures::{Future, StreamExt};
use gag::Gag;
use niketsu_core::config::Config;
use niketsu_core::file_database::{FileEntry, FileStore};
use niketsu_core::fuzzy::FuzzySearch;
use niketsu_core::playlist::Video;
use niketsu_core::playlist::file::PlaylistBrowser;
use niketsu_core::room::RoomName;
use niketsu_core::ui::{RoomChange, UiModel, UserInterface};
use ratatui::prelude::*;
use tokio::task::JoinHandle;
use tracing::warn;

use super::widget::playlist::PlaylistWidget;
use crate::handler::command::Command;
use crate::handler::help::Help;
use crate::handler::options::Options;
use crate::handler::playlist::Playlist;
use crate::handler::{EventHandler, MainEventHandler, OverlayState, RenderHandler, State};
use crate::widget::chat::{ChatWidget, ChatWidgetState};
use crate::widget::chat_input::{ChatInputWidget, ChatInputWidgetState};
use crate::widget::command::CommandInputWidgetState;
use crate::widget::database::{DatabaseWidget, DatabaseWidgetState};
use crate::widget::footer::{FooterWidget, FooterWidgetState};
use crate::widget::help::HelpWidgetState;
use crate::widget::login::LoginWidgetState;
use crate::widget::media::MediaDirWidgetState;
use crate::widget::options::OptionsWidgetState;
use crate::widget::playlist::PlaylistWidgetState;
use crate::widget::playlist::video_overlay::VideoNameWidgetState;
use crate::widget::playlist_browser::PlaylistBrowserWidgetState;
use crate::widget::recently::{RecentlyWidget, RecentlyWidgetState};
use crate::widget::search::SearchWidgetState;
use crate::widget::users::{UsersWidget, UsersWidgetState};

pub struct RatatuiView {
    pub app: App,
    pub model: UiModel,
    pub config: Config,
    pub running: bool,
}

enum LoopControl {
    Continue,
    Break,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum Mode {
    #[default]
    Normal,
    Inspecting,
    Overlay,
}

pub struct App {
    pub chat_widget_state: ChatWidgetState,
    pub database_widget_state: DatabaseWidgetState,
    pub users_widget_state: UsersWidgetState,
    pub playlist_widget_state: PlaylistWidgetState,
    pub command_input_widget_state: CommandInputWidgetState,
    pub chat_input_widget_state: ChatInputWidgetState,
    pub current_overlay_state: Option<OverlayState>,
    pub options_widget_state: OptionsWidgetState,
    pub help_widget_state: HelpWidgetState,
    pub login_widget_state: LoginWidgetState,
    pub media_widget_state: MediaDirWidgetState,
    pub browser_search_widget_state: SearchWidgetState<FileEntry, FileStore>,
    pub playlist_search_widget_state: SearchWidgetState<Video, niketsu_core::playlist::Playlist>,
    pub playlist_browser_widget_state: PlaylistBrowserWidgetState,
    pub video_name_widget_state: VideoNameWidgetState,
    pub recently_widget_state: RecentlyWidgetState,
    pub footer_widget_state: FooterWidgetState,
    pub current_browser_search: Option<FuzzySearch<FileEntry>>,
    pub current_playlist_search: Option<FuzzySearch<Video>>,
    pub clipboard: Option<Clipboard>,
    state: State,
    prev_state: Option<State>,
    mode: Mode,
    prev_mode: Option<Mode>,
}

impl App {
    fn new(config: Config) -> App {
        let clipboard = Clipboard::new().ok();
        let mut footer_widget_state = FooterWidgetState::default();
        footer_widget_state.set_content(&State::from(Playlist), &None, &Mode::Normal);

        App {
            chat_widget_state: ChatWidgetState::default(),
            database_widget_state: DatabaseWidgetState::default(),
            users_widget_state: UsersWidgetState::default(),
            playlist_widget_state: {
                let mut playlist_state = PlaylistWidgetState::default();
                playlist_state.set_style(Style::default().fg(Color::Magenta));
                playlist_state
            },
            command_input_widget_state: CommandInputWidgetState::default(),
            chat_input_widget_state: ChatInputWidgetState::new(),
            current_overlay_state: None,
            options_widget_state: OptionsWidgetState::default(),
            help_widget_state: HelpWidgetState::default(),
            login_widget_state: LoginWidgetState::new(&config),
            browser_search_widget_state: SearchWidgetState::new("Database Search".to_string()),
            media_widget_state: MediaDirWidgetState::new(config.media_dirs),
            playlist_search_widget_state: SearchWidgetState::new("Playlist Search".to_string()),
            playlist_browser_widget_state: PlaylistBrowserWidgetState::new(),
            video_name_widget_state: VideoNameWidgetState::default(),
            recently_widget_state: RecentlyWidgetState::new(),
            footer_widget_state,
            current_browser_search: None,
            current_playlist_search: None,
            clipboard,
            state: State::from(Playlist {}),
            prev_state: None,
            mode: Mode::Normal,
            prev_mode: None,
        }
    }

    pub fn set_current_state(&mut self, state: State) {
        self.state = state;
        self.set_footer();
    }

    pub fn set_current_overlay_state(&mut self, state: Option<OverlayState>) {
        self.current_overlay_state = state;
        self.set_footer();
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.prev_mode = Some(self.mode);
        self.mode = mode;
        self.set_footer();
        self.reset_help_state();
    }

    pub fn reset_overlay(&mut self) {
        self.current_overlay_state = None;
        if let Some(mode) = self.prev_mode {
            self.mode = mode;
        } else {
            self.mode = Mode::Normal;
        }
        self.set_footer();
    }

    pub fn reset_help_state(&mut self) {
        match self.mode {
            Mode::Normal => self.help_widget_state.reset(),
            Mode::Inspecting => self.help_widget_state.select(&self.state),
            Mode::Overlay => {}
        }
    }

    pub fn current_state(&self) -> State {
        self.state
    }

    pub fn search_browser(&mut self, query: String) {
        self.current_browser_search = self.browser_search_widget_state.fuzzy_search(query);
    }

    pub fn search_playlist(&mut self, query: String) {
        self.current_playlist_search = self.playlist_search_widget_state.fuzzy_search(query);
    }

    pub fn reset_browser_search(&mut self) {
        self.current_browser_search = None;
    }

    pub fn reset_playlist_search(&mut self) {
        self.current_playlist_search = None;
    }

    pub fn get_clipboard(&mut self) -> Result<String> {
        match &mut self.clipboard {
            Some(cb) => cb.get_text().map_err(|e| anyhow::anyhow!("{e:?}")),
            None => bail!("Clipboard not initialized"),
        }
    }

    pub fn set_footer(&mut self) {
        self.footer_widget_state
            .set_content(&self.state, &self.current_overlay_state, &self.mode);
    }
}

impl Drop for RatatuiView {
    fn drop(&mut self) {
        self.running = false;
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
            running: true,
        };
        let handle = Box::pin(async move { view.run().await });
        (ui, handle)
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = Self::setup_terminal().expect("tui setup failed");
        let mut event = EventStream::new();
        let mut tick_rate = tokio::time::interval(Duration::from_millis(50));
        let notify = self.model.notify.clone();
        let _suppress_stderr = Gag::stderr().unwrap();

        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            Self::restore_terminal().ok();
            original_hook(panic);
        }));

        let mut needs_update = false;
        terminal.draw(|f| Self::render(f, &mut self.app))?;
        let mut playlist_browser_handle: Option<JoinHandle<PlaylistBrowser>> =
            Some(tokio::task::spawn(async move {
                PlaylistBrowser::get_all().await
            }));

        while self.running {
            tokio::select! {
                ct_event = event.next() => {
                    if let LoopControl::Break = self.handle_event(ct_event) {
                        break;
                    }
                    needs_update = true;
                },
                Some(search_result) = OptionFuture::from(self.app.current_browser_search.as_mut()) => {
                    self.app.browser_search_widget_state.set_result(search_result);
                    self.app.current_browser_search = None;
                    needs_update = true;
                }
                Some(search_result) = OptionFuture::from(self.app.current_playlist_search.as_mut()) => {
                    self.app.playlist_search_widget_state.set_result(search_result);
                    self.app.current_playlist_search = None;
                    needs_update = true;
                }
               Some(result) = OptionFuture::from(playlist_browser_handle.as_mut()) => {
                    match result {
                        Ok(playlist_browser) => {
                            self.app.playlist_browser_widget_state.set_playlist_browser(playlist_browser);
                        },
                        Err(e) => {
                            warn!(?e, "Failed to retrieve playlists");
                        }
                    }
                    playlist_browser_handle = None;
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
                Mode::Inspecting => self.app.state.clone().handle_with_overlay(self, &event),
                Mode::Overlay => {
                    if let Some(overlay) = self.app.current_overlay_state.clone() {
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
                        self.app.command_input_widget_state.set_active(true);
                    }
                    KeyCode::Enter => {
                        self.app.set_mode(Mode::Inspecting);
                        self.highlight();
                    }
                    KeyCode::Char(' ') => {
                        self.app.set_mode(Mode::Overlay);
                        self.app
                            .set_current_overlay_state(Some(OverlayState::from(Options {})));
                    }
                    KeyCode::Char('?') => {
                        self.app.set_mode(Mode::Overlay);
                        self.app
                            .set_current_overlay_state(Some(OverlayState::from(Help {})));
                    }
                    KeyCode::Right | KeyCode::Left | KeyCode::Down | KeyCode::Up => {
                        self.app.state.clone().handle_next(self, key)
                    }
                    _ => {}
                }
            }
        }

        LoopControl::Continue
    }

    fn handle_notify(&mut self) {
        self.model.running.on_change(|running| {
            self.running = running;
        });

        self.model.file_database_status.on_change(|status| {
            self.app
                .database_widget_state
                .set_file_database_status((status * 100.0) as u16);
        });

        self.model.file_database.on_change(|db| {
            self.app.database_widget_state.set_file_database(db.clone());
            self.app.browser_search_widget_state.set_store(db.clone());
            self.app.recently_widget_state.set_file_database(db);
            let query = self.app.browser_search_widget_state.get_input();
            self.app.search_browser(query);
        });

        self.model.playlist.on_change(|playlist| {
            self.app
                .playlist_widget_state
                .set_playlist(playlist.clone());
            self.app.playlist_search_widget_state.set_store(playlist);
        });

        self.model.messages.on_change_arc(|messages| {
            self.app.chat_widget_state.set_messages(messages);
            self.app.chat_widget_state.update_cursor_latest();
        });

        self.model.user_list.on_change(|users| {
            self.app.users_widget_state.set_user_list(users);
        });

        self.model.user.on_change(|user| {
            self.app.users_widget_state.set_user(user.clone());
            self.app.chat_widget_state.set_user(user);
        });

        self.model.playing_video.on_change(|video| {
            self.app.playlist_widget_state.set_playing_video(video);
        });

        self.model.video_share.on_change(|sharing| {
            self.app.playlist_widget_state.set_video_share(sharing);
        });
    }

    fn render(f: &mut Frame, app: &mut App) {
        let area = f.area();
        let main_vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(area);

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(main_vertical_chunks[0]);

        let vertical_left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(25),
                    Constraint::Min(5),
                    Constraint::Length(4),
                ]
                .as_ref(),
            )
            .split(horizontal_chunks[0]);

        let vertical_right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Percentage(25),
                    Constraint::Min(3),
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
            RecentlyWidget,
            vertical_right_chunks[1],
            &mut app.recently_widget_state,
        );

        f.render_stateful_widget(
            PlaylistWidget,
            vertical_right_chunks[2],
            &mut app.playlist_widget_state,
        );

        f.render_stateful_widget(
            UsersWidget,
            vertical_left_chunks[0],
            &mut app.users_widget_state,
        );

        f.render_stateful_widget(
            ChatWidget {},
            vertical_left_chunks[1],
            &mut app.chat_widget_state,
        );

        f.render_stateful_widget(
            ChatInputWidget {},
            vertical_left_chunks[2],
            &mut app.chat_input_widget_state,
        );

        match (app.mode, &app.current_overlay_state) {
            (Mode::Overlay, Some(OverlayState::Command(overlay))) => {
                overlay.clone().render(f, app);
            }
            (Mode::Overlay, Some(overlay)) => {
                f.render_stateful_widget(
                    FooterWidget,
                    main_vertical_chunks[1],
                    &mut app.footer_widget_state,
                );
                overlay.clone().render(f, app);
            }
            _ => {
                f.render_stateful_widget(
                    FooterWidget,
                    main_vertical_chunks[1],
                    &mut app.footer_widget_state,
                );
            }
        }
    }

    pub fn transition(&mut self, to: State) {
        self.app.prev_state = Some(self.app.state);
        self.app
            .state
            .clone()
            .set_style(self, Style::default().fg(Color::default()));
        self.app.state = to;
        self.app
            .state
            .clone()
            .set_style(self, Style::default().fg(Color::Magenta));
    }

    pub fn highlight(&mut self) {
        self.app
            .state
            .clone()
            .set_style(self, Style::default().fg(Color::Cyan));
    }

    pub fn hover_highlight(&mut self) {
        self.app
            .state
            .clone()
            .set_style(self, Style::default().fg(Color::Magenta));
    }

    //TODO returning errors
    // hidden feature
    pub fn parse_commands(&mut self, msg: String) {
        //TODO refactor
        let args: Vec<&str> = msg.split_whitespace().collect();
        let args = args.as_slice();

        match args {
            ["w", msg @ ..] | ["write", msg @ ..] => self.model.send_message(msg.concat()),
            ["room-change", password, room] | ["sc", password, room] => {
                self.handle_room_change(password.to_string(), RoomName::from(*room))
            }
            ["room-change", room] | ["sc", room] => {
                self.handle_room_change(String::default(), RoomName::from(*room))
            }
            ["username-change", username] | ["uc", username] => {
                self.model.change_username(username.to_string().into())
            }
            ["toggle-ready"] | ["tr"] => self.model.user_ready_toggle(),
            ["start-update"] | ["load"] => self.model.start_db_update(),
            ["stop-update"] | ["stop"] => self.model.stop_db_update(),
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

    pub fn insert_range(&self, index: usize, videos: Vec<Video>) {
        let mut updated_playlist = self.model.playlist.get_inner();
        let saturated_index = std::cmp::min(index, updated_playlist.len());
        updated_playlist.insert_range(saturated_index, videos);
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    pub fn remove(&self, video: &Video) {
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

    pub fn reverse_range(&self, positions: (usize, usize)) {
        let mut updated_playlist = self.model.playlist.get_inner();
        updated_playlist.reverse_range(positions.0..=positions.1);
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
        self.model.playlist.set(updated_playlist.clone());
        self.model.change_playlist(updated_playlist);
    }

    pub fn select(&mut self, video: Video) {
        self.model.change_video(video)
    }

    pub fn change_media_dirs(&mut self, paths: Vec<PathBuf>) {
        self.model.change_db_paths(paths)
    }

    pub fn save_config(&mut self, password: String, room: RoomName, username: ArcStr) {
        self.config.password = password;
        self.config.room = room;
        self.config.username = username;
        _ = self.config.save();
    }

    pub fn save_media_dir(&mut self, paths: Vec<String>) {
        self.config.media_dirs = paths;
        _ = self.config.save();
    }

    fn handle_room_change(&mut self, password: String, room: RoomName) {
        self.model.change_room(RoomChange { password, room });
    }

    fn handle_move(&mut self, filename: &str, position: &str) {
        let position = match position.parse::<usize>() {
            Ok(value) => value,
            _ => return,
        };

        self.move_to(&Video::from(filename), position);
    }
}
