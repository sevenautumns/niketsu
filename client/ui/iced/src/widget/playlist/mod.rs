use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::event::Status;
use iced::keyboard::{KeyCode, Modifiers};
use iced::mouse::Cursor;
use iced::widget::{Button, Column, Container, Rule, Text};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme};
use log::trace;
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, *};

use self::message::*;
use crate::message::Message;
use crate::styling::{FileButton, FileRuleTheme};

pub mod message;

// TODO make configurable
pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct PlaylistWidget<'a> {
    base: Element<'a, Message>,
    state: PlaylistWidgetState,
}

impl<'a> PlaylistWidget<'a> {
    pub fn new(state: PlaylistWidgetState, playing: Option<Video>) -> Self {
        // TODO Add context menu

        let mut file_btns = vec![];
        for f in state.playlist.iter() {
            let pressed = state.selected.as_ref().map_or(false, |f_i| f.eq(f_i));
            let mut available = f.is_url();
            if !available {
                available = state.file_store.find_file(f.as_str()).is_some();
            }
            let mut name = f.as_str().to_string();
            if let Some(playing) = &playing {
                if name.eq(playing.as_str()) {
                    name = format!("> {name}");
                }
            };
            file_btns.push(
                Button::new(Container::new(Text::new(name)).padding(2))
                    .padding(0)
                    .width(Length::Fill)
                    .style(FileButton::theme(pressed, available))
                    .into(),
            );
        }

        Self {
            state,
            base: Column::with_children(file_btns).width(Length::Fill).into(),
        }
    }

    fn closest_index(
        &self,
        layout: iced::advanced::layout::Layout<'_>,
        cursor_position: Point,
    ) -> Option<(usize, Point)> {
        let files = layout.children();
        let mut closest = (f32::INFINITY, 0, iced::Point::default());
        // Find closest index from overlay
        for (i, layout) in files.enumerate() {
            let dist = layout.position().distance(cursor_position);
            if dist < closest.0 {
                closest = (dist, i, layout.position())
            }
        }
        // In-case we are at the end of the file list,
        // check if we are above or below
        if closest.1 == self.state.playlist.len() - 1 {
            if let Some(l) = layout.children().last() {
                let top = l.position();
                let mut bottom = top;
                bottom.y += l.bounds().height;
                let dist = bottom.distance(cursor_position);
                if dist < closest.0 {
                    closest = (dist, self.state.playlist.len(), bottom)
                }
            }
        }
        // If no index was found return None
        if closest.0.is_infinite() {
            return None;
        }
        // If we are below or above the selected file,
        // dont send an index if we handle files within the playlist
        if let FileInteraction::Pressing(_) = self.state.interaction {
            if let Some(sele) = &self.state.selected {
                if let Some(clos) = self.state.playlist.get(closest.1.saturating_sub(1)) {
                    if clos.eq(sele) {
                        return None;
                    }
                }
                if let Some(clos) = self.state.playlist.get(closest.1) {
                    if clos.eq(sele) {
                        return None;
                    }
                }
            }
        }
        Some((closest.1, closest.2))
    }

    fn file_at_position(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor_position: Point,
    ) -> Option<Video> {
        let files = self.state.playlist.iter().zip(layout.children());
        for (file, lay) in files {
            if lay.bounds().contains(cursor_position) {
                return Some(file.clone());
            }
        }

        None
    }

    fn pressed(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor_position: Point,
        shell: &mut iced::advanced::Shell<'_, Message>,
    ) {
        if let Some(file) = self.file_at_position(layout, cursor_position) {
            // if let Some(i) = self.state.file_index(&file) {
            let interaction = FileInteraction::Pressing(Instant::now());
            shell.publish(
                PlaylistWidgetMessage::from(Interaction {
                    video: Some(file.clone()),
                    interaction,
                })
                .into(),
            );

            if let Some(prev_file) = &self.state.selected {
                if let FileInteraction::Released(when) = self.state.interaction {
                    if file.eq(prev_file) && when.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                        shell.publish(
                            PlaylistWidgetMessage::from(DoubleClick { video: file }).into(),
                        );
                    }
                }
                // }
            }
        }
    }

    fn released(
        &self,
        file: Option<PathBuf>,
        state: &InnerState,
        layout: iced::advanced::layout::Layout<'_>,
        shell: &mut iced::advanced::Shell<'_, Message>,
    ) {
        match &self.state.interaction {
            FileInteraction::PressingExternal => {
                if let Some(name) = file.and_then(|f| {
                    f.file_name()
                        .and_then(|f| f.to_str().map(|f| f.to_string()))
                }) {
                    shell.publish(
                        PlaylistWidgetMessage::from(Move {
                            video: Video::from(name.as_str()),
                            pos: 0,
                        })
                        .into(),
                    );
                }
                // }
            }
            FileInteraction::Pressing(_) => {
                let pos = state.cursor_position;
                if let Some((i, _)) = self.closest_index(layout, pos) {
                    if let Some(file) = &self.state.selected {
                        shell.publish(
                            PlaylistWidgetMessage::from(Move {
                                video: file.clone(),
                                pos: i,
                            })
                            .into(),
                        )
                    }
                }
                shell.publish(
                    PlaylistWidgetMessage::from(Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::Released(Instant::now()),
                    })
                    .into(),
                )
            }
            FileInteraction::Released(_) => {
                shell.publish(
                    PlaylistWidgetMessage::from(Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::None,
                    })
                    .into(),
                );
            }
            FileInteraction::None => (),
        }
    }

    fn deleted(&self, shell: &mut iced::advanced::Shell<'_, Message>) {
        if let Some(f) = &self.state.selected {
            shell.publish(PlaylistWidgetMessage::from(Delete { video: f.clone() }).into())
        }
    }
}

impl<'a> iced::advanced::Widget<Message, Renderer> for PlaylistWidget<'a> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<InnerState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(InnerState::default())
    }

    fn width(&self) -> Length {
        self.base.as_widget().width()
    }

    fn height(&self) -> Length {
        self.base.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base.as_widget().layout(renderer, limits)
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        // Draw insert_hint
        if self.state.interaction.is_press() {
            let inner_state = state.state.downcast_ref::<InnerState>();
            if let Some((_, pos)) = self.closest_index(layout, inner_state.cursor_position) {
                InsertHint::new(pos).draw(renderer, theme, style, layout, cursor)
            }
        }
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::layout::Layout<'_>,
        cursor: Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::mouse::Interaction {
        if self.state.interaction.is_press() || self.state.interaction.is_press_extern() {
            return iced::mouse::Interaction::Grabbing;
        }

        self.base.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn on_event(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) -> Status {
        let mut _status = iced::event::Status::Ignored;
        let inner_state = state.state.downcast_mut::<InnerState>();

        // Workaround for if we touch the overlay
        // trace!("{cursor_position:?}")
        if let Cursor::Available(cursor_position) = cursor {
            inner_state.cursor_position = cursor_position;
        }

        match &event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key_code,
                modifiers,
            }) => {
                _status = iced::event::Status::Captured;
                // TODO arrow keys
                if modifiers.is_empty() && *key_code == KeyCode::Delete {
                    self.deleted(shell)
                }
                // TODO use File input instead
                if modifiers.contains(Modifiers::CTRL) && *key_code == KeyCode::V {
                    if let Some(clipboard) = clipboard.read() {
                        shell.publish(
                            PlaylistWidgetMessage::from(Move {
                                video: Video::from(clipboard.as_str()),
                                pos: 0,
                            })
                            .into(),
                        )
                    }
                }
            }
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                    if let Cursor::Available(cursor_position) = cursor {
                        self.pressed(layout, cursor_position, shell)
                    }
                }
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                    self.released(None, inner_state, layout, shell)
                }
                _ => {}
            },
            iced::Event::Touch(t) => {
                _status = iced::event::Status::Captured;
                match t {
                    iced::touch::Event::FingerPressed { id: _, position } => {
                        self.pressed(layout, *position, shell)
                    }
                    iced::touch::Event::FingerLifted { id: _, position: _ } => {
                        self.released(None, inner_state, layout, shell)
                    }
                    _ => {}
                }
            }
            iced::Event::Window(event) => match event {
                iced::window::Event::FileHovered(_) => {
                    if !self.state.interaction.is_press_extern() {
                        shell.publish(
                            PlaylistWidgetMessage::from(Interaction {
                                video: self.state.selected.clone(),
                                interaction: FileInteraction::PressingExternal,
                            })
                            .into(),
                        )
                    }
                }
                iced::window::Event::FileDropped(file) => {
                    trace!("file dropped: {file:?}");
                    self.released(Some(file.clone()), inner_state, layout, shell)
                }
                iced::window::Event::FilesHoveredLeft => shell.publish(
                    PlaylistWidgetMessage::from(Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::None,
                    })
                    .into(),
                ),
                _ => {}
            },
            _ => {}
        }

        let inner_status = self.base.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        // match status {
        //     iced::event::Status::Ignored => inner_status,
        //     iced::event::Status::Captured => status,
        // }
        // TODO properly figure out if we captured something or not
        inner_status
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Renderer>> {
        self.base.as_widget_mut().overlay(state, layout, renderer)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct InnerState {
    cursor_position: iced::Point,
}

#[derive(Debug, Default, Clone)]
pub struct PlaylistWidgetState {
    playlist: Playlist,
    file_store: FileStore,
    selected: Option<Video>,
    interaction: FileInteraction,
}

#[derive(Debug, Clone, Default)]
pub enum FileInteraction {
    PressingExternal,
    Pressing(Instant),
    Released(Instant),
    #[default]
    None,
}

impl FileInteraction {
    pub fn is_press_extern(&self) -> bool {
        matches!(self, FileInteraction::PressingExternal)
    }
    pub fn is_press(&self) -> bool {
        matches!(self, FileInteraction::Pressing(_))
    }
    pub fn is_released(&self) -> bool {
        matches!(self, FileInteraction::Released(_))
    }
    pub fn is_none(&self) -> bool {
        matches!(self, FileInteraction::None)
    }
}

impl PlaylistWidgetState {
    pub fn move_video(&mut self, video: &Video, index: usize) {
        self.playlist.move_video(video, index);
    }

    pub fn file_interaction(&mut self, video: Option<Video>, interaction: FileInteraction) {
        self.selected = video;
        self.interaction = interaction;
    }

    pub fn delete_video(&mut self, video: &Video) {
        self.playlist.remove_by_video(video);
    }

    pub fn replace_playlist(&mut self, playlist: Playlist) {
        self.playlist = playlist;

        if let Some(video) = &self.selected {
            if self.playlist.find(video).is_none() {
                self.selected = None;
                self.interaction = FileInteraction::None;
            }
        }
    }

    pub fn update_file_store(&mut self, store: FileStore) {
        self.file_store = store
    }

    pub fn video_index(&self, video: &Video) -> Option<usize> {
        for (i, v) in self.playlist.iter().enumerate() {
            if v.eq(video) {
                return Some(i);
            }
        }
        None
    }
}

impl<'a> From<PlaylistWidget<'a>> for Element<'a, Message> {
    fn from(table: PlaylistWidget<'a>) -> Self {
        Self::new(table)
    }
}

pub struct InsertHint {
    rule: Rule<Renderer>,
    pos: iced::Point,
}

impl Default for InsertHint {
    fn default() -> Self {
        Self {
            rule: Rule::horizontal(1).style(FileRuleTheme::theme()),
            pos: iced::Point::default(),
        }
    }
}

impl InsertHint {
    pub fn new(pos: iced::Point) -> Self {
        Self {
            pos,
            ..Default::default()
        }
    }

    pub fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
    ) {
        let limits = iced::advanced::layout::Limits::new(Size::ZERO, layout.bounds().size())
            .width(Length::Fill)
            .height(1);
        let mut node = <iced::widget::Rule<Renderer> as iced::advanced::Widget<
            Message,
            Renderer,
        >>::layout(&self.rule, renderer, &limits);
        node.move_to(self.pos);
        let layout = iced::advanced::Layout::new(&node);
        <iced::widget::Rule<Renderer> as iced::advanced::Widget<Message, Renderer>>::draw(
            &self.rule,
            &iced::advanced::widget::Tree::empty(),
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        )
    }
}
