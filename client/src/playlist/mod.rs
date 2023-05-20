use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::{KeyCode, Modifiers};
use iced::widget::{Button, Column, Container, Rule, Text};
use iced::{Element, Length, Renderer, Size};
use iced_native::widget::Tree;
use iced_native::{Layout, Widget};
use log::*;

use self::message::{Delete, DoubleClick, Interaction, Move, PlaylistMessage};
use crate::fs::FileDatabase;
use crate::iced_window::MainMessage;
use crate::styling::{FileButton, FileRuleTheme};
use crate::video::{PlayingFile, Video};

pub mod message;

// TODO make configurable
pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct PlaylistWidget<'a> {
    base: Element<'a, MainMessage>,
    state: PlaylistWidgetState,
}

impl<'a> PlaylistWidget<'a> {
    pub fn new(
        state: PlaylistWidgetState,
        playing: Option<PlayingFile>,
        db: &Arc<FileDatabase>,
    ) -> Self {
        // TODO Add context menu

        let mut file_btns = vec![];
        for f in state.videos.iter() {
            let pressed = state.selected.as_ref().map_or(false, |f_i| f.eq(f_i));
            let mut available = f.is_url();
            if !available {
                available = db.find_file(f.as_str()).ok().flatten().is_some();
            }
            let mut name = f.as_str().to_string();
            if let Some(playing) = &playing {
                if name.eq(playing.video.as_str()) {
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
}

impl<'a> PlaylistWidget<'a> {
    fn closest_index(
        &self,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
    ) -> Option<(usize, iced::Point)> {
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
        if closest.1 == self.state.videos.len() - 1 {
            if let Some(l) = layout.children().last() {
                let top = l.position();
                let mut bottom = top;
                bottom.y += l.bounds().height;
                let dist = bottom.distance(cursor_position);
                if dist < closest.0 {
                    closest = (dist, self.state.videos.len(), bottom)
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
                if let Some(clos) = self.state.videos.get(closest.1.saturating_sub(1)) {
                    if clos.eq(sele) {
                        return None;
                    }
                }
                if let Some(clos) = self.state.videos.get(closest.1) {
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
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
    ) -> Option<Video> {
        let files = self.state.videos.iter().zip(layout.children());
        for (file, lay) in files {
            if lay.bounds().contains(cursor_position) {
                return Some(file.clone());
            }
        }

        None
    }

    fn pressed(
        &self,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
        shell: &mut iced_native::Shell<'_, MainMessage>,
    ) {
        if let Some(file) = self.file_at_position(layout, cursor_position) {
            // if let Some(i) = self.state.file_index(&file) {
            let interaction = FileInteraction::Pressing(Instant::now());
            shell.publish(
                PlaylistMessage::from(Interaction {
                    video: Some(file.clone()),
                    interaction,
                })
                .into(),
            );

            if let Some(prev_file) = &self.state.selected {
                if let FileInteraction::Released(when) = self.state.interaction {
                    if file.eq(prev_file) && when.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                        shell.publish(PlaylistMessage::from(DoubleClick { video: file }).into());
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
        layout: iced_native::Layout<'_>,
        shell: &mut iced_native::Shell<'_, MainMessage>,
    ) {
        match &self.state.interaction {
            FileInteraction::PressingExternal => {
                if let Some(name) = file.and_then(|f| {
                    f.file_name()
                        .and_then(|f| f.to_str().map(|f| f.to_string()))
                }) {
                    shell.publish(
                        PlaylistMessage::from(Move {
                            video: Video::from_string(name),
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
                            PlaylistMessage::from(Move {
                                video: file.clone(),
                                pos: i,
                            })
                            .into(),
                        )
                    }
                }
                shell.publish(
                    PlaylistMessage::from(Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::Released(Instant::now()),
                    })
                    .into(),
                )
            }
            FileInteraction::Released(_) => {
                shell.publish(
                    PlaylistMessage::from(Interaction {
                        video: self.state.selected.clone(),
                        interaction: FileInteraction::None,
                    })
                    .into(),
                );
            }
            FileInteraction::None => (),
        }
    }

    fn deleted(&self, shell: &mut iced_native::Shell<'_, MainMessage>) {
        if let Some(f) = &self.state.selected {
            shell.publish(PlaylistMessage::from(Delete { video: f.clone() }).into())
        }
    }
}

impl<'a> Widget<MainMessage, Renderer> for PlaylistWidget<'a> {
    fn tag(&self) -> iced_native::widget::tree::Tag {
        iced_native::widget::tree::Tag::of::<InnerState>()
    }

    fn state(&self) -> iced_native::widget::tree::State {
        iced_native::widget::tree::State::new(InnerState::default())
    }

    fn width(&self) -> iced_native::Length {
        self.base.as_widget().width()
    }

    fn height(&self) -> iced_native::Length {
        self.base.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced_native::layout::Limits,
    ) -> iced_native::layout::Node {
        self.base.as_widget().layout(renderer, limits)
    }

    fn draw(
        &self,
        state: &iced_native::widget::Tree,
        renderer: &mut Renderer,
        theme: &<Renderer as iced_native::Renderer>::Theme,
        style: &iced_native::renderer::Style,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
        viewport: &iced_native::Rectangle,
    ) {
        // Add insert hint here
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor_position,
            viewport,
        );
        // Draw insert_hint
        if self.state.interaction.is_press() {
            let inner_state = state.state.downcast_ref::<InnerState>();
            if let Some((_, pos)) = self.closest_index(layout, inner_state.cursor_position) {
                InsertHint::new(pos).draw(renderer, theme, style, layout, cursor_position)
            }
        }
    }

    fn children(&self) -> Vec<iced_native::widget::Tree> {
        vec![Tree::new(&self.base)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.base))
    }

    fn operate(
        &self,
        state: &mut Tree,
        layout: iced_native::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced_native::widget::Operation<MainMessage>,
    ) {
        self.base
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: iced_native::Layout<'_>,
        cursor_position: iced::Point,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced_native::mouse::Interaction {
        //TODO Change mouse interaction

        self.base.as_widget().mouse_interaction(
            &state.children[0],
            layout,
            cursor_position,
            viewport,
            renderer,
        )
    }

    fn on_event(
        &mut self,
        state: &mut Tree,
        event: iced_native::Event,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
        renderer: &Renderer,
        clipboard: &mut dyn iced_native::Clipboard,
        shell: &mut iced_native::Shell<'_, MainMessage>,
    ) -> iced_native::event::Status {
        let mut _status = iced_native::event::Status::Ignored;
        let inner_state = state.state.downcast_mut::<InnerState>();

        // Workaround for if we touch the overlay
        // trace!("{cursor_position:?}")
        if cursor_position.x.is_sign_positive() && cursor_position.y.is_sign_positive() {
            inner_state.cursor_position = cursor_position;
        }

        match &event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key_code,
                modifiers,
            }) => {
                _status = iced_native::event::Status::Captured;
                // TODO arrow keys
                if modifiers.is_empty() && *key_code == KeyCode::Delete {
                    self.deleted(shell)
                }
                // TODO use File input instead
                if modifiers.contains(Modifiers::CTRL) && *key_code == KeyCode::V {
                    if let Some(clipboard) = clipboard.read() {
                        shell.publish(
                            PlaylistMessage::from(Move {
                                video: Video::from_string(clipboard),
                                pos: 0,
                            })
                            .into(),
                        )
                    }
                }
            }
            iced::Event::Mouse(event) => match event {
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                    self.pressed(layout, cursor_position, shell)
                }
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                    self.released(None, inner_state, layout, shell)
                }
                _ => {}
            },
            iced::Event::Touch(t) => {
                _status = iced_native::event::Status::Captured;
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
                            PlaylistMessage::from(Interaction {
                                video: self.state.selected.clone(),
                                interaction: FileInteraction::PressingExternal,
                            })
                            .into(),
                        )
                    }
                }
                iced::window::Event::FileDropped(file) => {
                    trace!("File dropped: {file:?}");
                    self.released(Some(file.clone()), inner_state, layout, shell)
                }
                iced::window::Event::FilesHoveredLeft => shell.publish(
                    PlaylistMessage::from(Interaction {
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
            cursor_position,
            renderer,
            clipboard,
            shell,
        );

        // match status {
        //     iced::event::Status::Ignored => inner_status,
        //     iced::event::Status::Captured => status,
        // }
        // TODO properly figure out if we captured something or not
        inner_status
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct InnerState {
    cursor_position: iced::Point,
}

#[derive(Debug, Default, Clone)]
pub struct PlaylistWidgetState {
    //TODO
    // active: bool,
    videos: Vec<Video>,
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
    pub fn move_video(&mut self, video: Video, index: usize) {
        let mut index = index;
        if let Some(i) = self.video_index(&video) {
            if index > i {
                index -= 1
            }
            self.videos.remove(i);
            self.videos.insert(index, video)
        } else {
            self.videos.push(video)
        }
    }

    pub fn videos(&self) -> Vec<Video> {
        self.videos.clone()
    }

    pub fn next_video(&self, video: &Video) -> Option<Video> {
        if let Some(i) = self.video_index(video) {
            return self.videos.get(i + 1).cloned();
        }
        None
    }

    pub fn file_interaction(&mut self, video: Option<Video>, interaction: FileInteraction) {
        self.selected = video;
        self.interaction = interaction;
    }

    pub fn delete_video(&mut self, video: &Video) {
        // TODO on delete move selected file to file above (or below if non are above)
        if let Some(i) = self.video_index(video) {
            self.videos.remove(i);
        }
    }

    pub fn replace_videos(&mut self, videos: Vec<String>) {
        let mut videos = videos;
        self.videos = videos.drain(..).map(Video::from_string).collect();
        // TODO If last pressed file does not exist anymore, change interaction
        // if let Some((file, _)) = &self.last_press {
        //     if !self.files.iter().any(|f| file.uuid.eq(&f.uuid)) {
        //         self.last_press = None;
        //         self.pressing = false;
        //     }
        // }
        self.interaction = FileInteraction::None;
    }

    pub fn video_index(&self, video: &Video) -> Option<usize> {
        for (i, v) in self.videos.iter().enumerate() {
            if v.eq(video) {
                return Some(i);
            }
        }
        None
    }
}

impl<'a> From<PlaylistWidget<'a>> for Element<'a, MainMessage> {
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
        theme: &<Renderer as iced_native::Renderer>::Theme,
        style: &iced_native::renderer::Style,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
    ) {
        let limits = iced_native::layout::Limits::new(Size::ZERO, layout.bounds().size())
            .width(Length::Fill)
            .height(1);
        let mut node = <iced::widget::Rule<Renderer> as Widget<MainMessage, Renderer>>::layout(
            &self.rule, renderer, &limits,
        );
        node.move_to(self.pos);
        let layout = Layout::new(&node);
        <iced::widget::Rule<Renderer> as iced_native::Widget<MainMessage, Renderer>>::draw(
            &self.rule,
            &Tree::empty(),
            renderer,
            theme,
            style,
            layout,
            cursor_position,
            &layout.bounds(),
        )
    }
}
