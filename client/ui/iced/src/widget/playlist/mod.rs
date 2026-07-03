use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::advanced::widget::Operation;
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::mouse::Cursor;
use iced::widget::text::Wrapping;
use iced::widget::{Column, Container, button, row, text};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};
use niketsu_core::file_database::FileStore;
use niketsu_core::playlist::{Playlist, *};
use tracing::trace;

use self::message::*;
use crate::TEXT_SIZE;
use crate::message::Message;
use crate::styling::{ModalContainer, PlaylistEntry};

pub mod message;

// TODO make configurable
pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
pub const PLAYLIST_SPACING: f32 = 2.0;

/// How far the cursor has to travel from the press before it counts as
/// a drag (shows the insert hint, moves the entry on release).
const DRAG_THRESHOLD: f32 = 5.0;
/// Height of the zones at the top and bottom of the visible playlist
/// that trigger autoscrolling while dragging.
const AUTOSCROLL_EDGE: f32 = 30.0;
/// Maximum autoscroll speed in pixels per frame.
const AUTOSCROLL_SPEED: f32 = 8.0;

/// The actions modal opened by right-clicking a playlist entry.
pub fn context_view(state: &PlaylistWidgetState) -> Element<'_, Message> {
    let video = state
        .context
        .as_ref()
        .map(|v| v.video.as_str())
        .unwrap_or_default();
    let play_button = button(
        text("Play")
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .on_press(ContextPlay.into())
    .width(Length::Fill);
    let remove_button = button(
        text("Remove")
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .on_press(ContextRemove.into())
    .width(Length::Fill)
    .style(iced::widget::button::danger);
    let close_button = button("Close")
        .on_press(CloseContext.into())
        .style(iced::widget::button::secondary);
    let top_row = row!(text(video).width(Length::Fill), close_button).spacing(5);
    let base = Column::new()
        .push(top_row)
        .push(play_button)
        .push(remove_button)
        .spacing(5)
        .padding(5)
        .width(Length::Fixed(250.0));

    let base: Element<'_, PlaylistWidgetMessage> =
        Container::new(base).style(ModalContainer::theme).into();
    base.map(Message::from)
}

pub struct PlaylistWidget<'a> {
    base: Element<'a, PlaylistWidgetMessage>,
    state: &'a PlaylistWidgetState,
}

impl<'a> PlaylistWidget<'a> {
    pub fn new(state: &'a PlaylistWidgetState, playing: Option<Video>) -> Self {
        let mut file_btns = vec![];
        for f in state.playlist.iter() {
            let pressed = state.selected.as_ref().is_some_and(|f_i| f.eq(&f_i.video));
            let mut available = f.is_url();
            if !available {
                available = state.file_store.find_file(f.as_str()).is_some();
            }
            let is_playing = playing.as_ref().is_some_and(|p| f.eq(p));
            // A fixed-width marker slot, so labels don't shift when
            // playback moves to another entry.
            let marker = text(if is_playing { "▶" } else { "" })
                .width(Length::Fixed(TEXT_SIZE + 2.0))
                .align_x(iced::alignment::Horizontal::Center);
            let label = text(f.as_str().to_string()).wrapping(Wrapping::None);
            file_btns.push(
                button(row!(marker, label))
                    .clip(true)
                    .padding(2)
                    .style(PlaylistEntry::theme(pressed, available, is_playing))
                    .into(),
            );
        }

        Self {
            state,
            base: Column::with_children(file_btns)
                .spacing(PLAYLIST_SPACING)
                .clip(true)
                // .width(Length::Fill)
                .into(),
        }
    }

    fn closest_index(
        &self,
        layout: iced::advanced::layout::Layout<'_>,
        cursor_position: Point,
    ) -> Option<Index> {
        let files = layout.children();
        let mut closest = (f32::INFINITY, Index::default());
        // The insertion slots are the top edges of the rows plus one
        // below the last row; compare vertical distance only, so a
        // cursor near the right edge of a row doesn't snap elsewhere.
        for (i, layout) in files.enumerate() {
            let dist = (layout.position().y - cursor_position.y).abs();
            if dist < closest.0 {
                closest.0 = dist;
                closest.1.index_absolute = i;
                closest.1.index_relative = i;
                closest.1.position = layout.position();
            }
        }
        if let Some(l) = layout.children().last() {
            let mut bottom = l.position();
            bottom.y += l.bounds().height;
            let dist = (bottom.y - cursor_position.y).abs();
            if dist < closest.0 {
                closest.0 = dist;
                closest.1.index_absolute = self.state.playlist.len();
                closest.1.index_relative = self.state.playlist.len();
                closest.1.position = bottom;
            }
        }
        // If the closest index is larger than the index of the selected video, adjust the relative index
        if self
            .state
            .selected
            .as_ref()
            .is_some_and(|s| s.index < closest.1.index_absolute)
        {
            closest.1.index_relative = closest.1.index_absolute - 1;
        }
        // If no index was found return None
        if closest.0.is_infinite() {
            return None;
        }
        // If we are below or above the selected file, dont send an index
        if let FileInteraction::Pressing(_) = self.state.interaction
            && let Some(sele) = &self.state.selected
        {
            if let Some(clos) = self
                .state
                .playlist
                .get(closest.1.index_absolute.saturating_sub(1))
                && clos.eq(&sele.video)
            {
                return None;
            }
            if let Some(clos) = self.state.playlist.get(closest.1.index_absolute)
                && clos.eq(&sele.video)
            {
                return None;
            }
        }
        Some(closest.1)
    }

    fn file_at_position(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor_position: Point,
    ) -> Option<VideoIndex> {
        let files = self.state.playlist.iter().zip(layout.children());
        for (index, (file, lay)) in files.enumerate() {
            if lay.bounds().contains(cursor_position) {
                return Some(VideoIndex {
                    index,
                    video: file.clone(),
                });
            }
        }

        None
    }

    fn pressed(
        &self,
        layout: iced::advanced::Layout<'_>,
        cursor_position: Point,
        shell: &mut iced::advanced::Shell<'_, PlaylistWidgetMessage>,
    ) {
        if let Some(file) = self.file_at_position(layout, cursor_position) {
            // if let Some(i) = self.state.file_index(&file) {
            let interaction = FileInteraction::Pressing(Instant::now());
            shell.publish(
                Interaction {
                    video: Some(file.clone()),
                    interaction,
                }
                .into(),
            );

            if let Some(prev_file) = &self.state.selected
                && let FileInteraction::Released(when) = self.state.interaction
                && file.eq(prev_file)
                && when.elapsed() < MAX_DOUBLE_CLICK_INTERVAL
            {
                shell.publish(DoubleClick { video: file.video }.into());
            }
        }
    }

    fn released(
        &self,
        file: Option<PathBuf>,
        dragged: bool,
        state: &InnerState,
        layout: iced::advanced::layout::Layout<'_>,
        shell: &mut iced::advanced::Shell<'_, PlaylistWidgetMessage>,
    ) {
        match &self.state.interaction {
            FileInteraction::PressingExternal => {
                if let Some(name) = file.and_then(|f| {
                    f.file_name()
                        .and_then(|f| f.to_str().map(|f| f.to_string()))
                }) {
                    shell.publish(
                        Move {
                            video: Video::from(name.as_str()),
                            pos: 0,
                        }
                        .into(),
                    );
                }
            }
            FileInteraction::Pressing(_) => {
                if dragged
                    && let Some(Index {
                        index_relative: pos,
                        ..
                    }) = self.closest_index(layout, state.cursor_position)
                    && let Some(file) = &self.state.selected
                {
                    shell.publish(
                        Move {
                            video: file.clone().video,
                            pos,
                        }
                        .into(),
                    )
                }
                shell.publish(
                    Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::Released(Instant::now()),
                    }
                    .into(),
                )
            }
            FileInteraction::Released(_) => {
                shell.publish(
                    Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::None,
                    }
                    .into(),
                );
            }
            FileInteraction::None => (),
        }
    }

    /// Whether a pressed entry has been dragged past [`DRAG_THRESHOLD`].
    fn drag_active(&self, inner: &InnerState) -> bool {
        self.state.interaction.is_press()
            && inner
                .press_position
                .is_some_and(|origin| origin.distance(inner.cursor_position) > DRAG_THRESHOLD)
    }

    fn deleted(&self, shell: &mut iced::advanced::Shell<'_, PlaylistWidgetMessage>) {
        if let Some(f) = &self.state.selected {
            shell.publish(
                Delete {
                    video: f.video.clone(),
                }
                .into(),
            )
        }
    }
}

impl iced::advanced::Widget<PlaylistWidgetMessage, Theme, Renderer> for PlaylistWidget<'_> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<InnerState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(InnerState::default())
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        // Draw insert hint
        let inner_state = state.state.downcast_ref::<InnerState>();
        if self.drag_active(inner_state)
            && let Some(Index { position: pos, .. }) =
                self.closest_index(layout, inner_state.cursor_position)
        {
            // Move point up by half the spacing
            let pos = Point {
                y: pos.y - (PLAYLIST_SPACING / 2.0),
                ..pos
            };
            iced::advanced::Renderer::fill_quad(
                renderer,
                iced::advanced::renderer::Quad {
                    bounds: Rectangle::new(pos, Size::new(layout.bounds().width, 1.0)),
                    ..Default::default()
                },
                theme.palette().text,
            );
        }
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.base
            .as_widget_mut()
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
        let inner_state = state.state.downcast_ref::<InnerState>();
        if self.drag_active(inner_state) || self.state.interaction.is_press_extern() {
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

    fn update(
        &mut self,
        state: &mut iced::advanced::widget::Tree,
        event: &Event,
        layout: iced::advanced::Layout<'_>,
        cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, PlaylistWidgetMessage>,
        viewport: &iced::Rectangle,
    ) {
        let inner_state = state.state.downcast_mut::<InnerState>();

        // Workaround for if we touch the overlay
        if let Cursor::Available(cursor_position) = cursor {
            inner_state.cursor_position = cursor_position;
        }

        match &event {
            // Widgets earlier in the tree (e.g. a focused chat input)
            // may have captured the key press already; leave it alone.
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. })
                if !shell.is_event_captured() =>
            {
                // TODO arrow keys
                if modifiers.is_empty() && *key == Key::Named(Named::Delete) {
                    self.deleted(shell)
                }
                // TODO use File input instead
                if modifiers.command()
                    && key.as_ref() == Key::Character("v")
                    && let Some(clipboard) =
                        clipboard.read(iced::advanced::clipboard::Kind::Standard)
                {
                    shell.publish(
                        Move {
                            video: Video::from(clipboard.as_str()),
                            pos: 0,
                        }
                        .into(),
                    )
                }
            }
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                    if let Cursor::Available(cursor_position) = cursor {
                        inner_state.press_position = Some(cursor_position);
                        self.pressed(layout, cursor_position, shell)
                    }
                }
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                    let dragged = self.drag_active(inner_state);
                    inner_state.press_position = None;
                    self.released(None, dragged, inner_state, layout, shell)
                }
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right) => {
                    if let Cursor::Available(cursor_position) = cursor
                        && let Some(file) = self.file_at_position(layout, cursor_position)
                    {
                        shell.publish(OpenContext { video: file }.into());
                    }
                }
                _ => {}
            },
            iced::Event::Touch(t) => match t {
                iced::touch::Event::FingerPressed { id: _, position } => {
                    inner_state.press_position = Some(*position);
                    self.pressed(layout, *position, shell)
                }
                iced::touch::Event::FingerLifted { id: _, position: _ } => {
                    let dragged = self.drag_active(inner_state);
                    inner_state.press_position = None;
                    self.released(None, dragged, inner_state, layout, shell)
                }
                _ => {}
            },
            iced::Event::Window(event) => match event {
                iced::window::Event::FileHovered(_)
                    if !self.state.interaction.is_press_extern() =>
                {
                    shell.publish(
                        Interaction {
                            video: self.state.selected.clone(),
                            interaction: FileInteraction::PressingExternal,
                        }
                        .into(),
                    )
                }
                iced::window::Event::FileDropped(file) => {
                    trace!(?file, "file dropped");
                    self.released(Some(file.clone()), false, inner_state, layout, shell)
                }
                iced::window::Event::FilesHoveredLeft => shell.publish(
                    Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::None,
                    }
                    .into(),
                ),
                // Scroll the surrounding Scrollable while a dragged
                // entry hovers near the edge of the visible area.
                iced::window::Event::RedrawRequested(_) if self.drag_active(inner_state) => {
                    let cursor_y = inner_state.cursor_position.y;
                    let top = viewport.y + AUTOSCROLL_EDGE;
                    let bottom = viewport.y + viewport.height - AUTOSCROLL_EDGE;
                    let overshoot = if cursor_y < top {
                        cursor_y - top
                    } else if cursor_y > bottom {
                        cursor_y - bottom
                    } else {
                        0.0
                    };
                    if overshoot != 0.0 {
                        let delta =
                            (overshoot / AUTOSCROLL_EDGE).clamp(-1.0, 1.0) * AUTOSCROLL_SPEED;
                        shell.publish(AutoScroll { delta }.into());
                    }
                }
                _ => {}
            },
            _ => {}
        }

        self.base.as_widget_mut().update(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        shell.request_redraw();
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'b>,
        renderer: &Renderer,
        rectangle: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, PlaylistWidgetMessage, Theme, Renderer>> {
        self.base.as_widget_mut().overlay(
            &mut state.children[0],
            layout,
            renderer,
            rectangle,
            translation,
        )
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct InnerState {
    cursor_position: iced::Point,
    /// Where the current press started; used for the drag threshold.
    press_position: Option<iced::Point>,
}

#[derive(Debug, Default, Clone)]
pub struct PlaylistWidgetState {
    playlist: Playlist,
    file_store: FileStore,
    selected: Option<VideoIndex>,
    interaction: FileInteraction,
    /// The entry whose right-click actions modal is open.
    context: Option<VideoIndex>,
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
}

impl PlaylistWidgetState {
    pub fn move_video(&mut self, video: &Video, index: usize) {
        self.playlist.move_video(video, index);
    }

    pub fn file_interaction(&mut self, video: Option<VideoIndex>, interaction: FileInteraction) {
        self.selected = video;
        self.interaction = interaction;
    }

    pub fn delete_video(&mut self, video: &Video) {
        self.playlist.remove_by_video(video);
    }

    pub fn replace_playlist(&mut self, playlist: Playlist) {
        self.playlist = playlist;

        if let Some(video) = &self.selected
            && self.playlist.find(&video.video).is_none()
        {
            self.selected = None;
            self.interaction = FileInteraction::None;
        }
    }

    pub fn update_file_store(&mut self, store: FileStore) {
        self.file_store = store
    }

    pub fn context_active(&self) -> bool {
        self.context.is_some()
    }
}

impl<'a> From<PlaylistWidget<'a>> for Element<'a, Message> {
    fn from(table: PlaylistWidget<'a>) -> Self {
        Element::new(table).map(Message::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoIndex {
    pub index: usize,
    pub video: Video,
}

#[derive(Default)]
struct Index {
    /// The index, which is used by the insert hint
    index_absolute: usize,
    /// The index, which is used by the playlist for moving
    index_relative: usize,
    position: Point,
}
