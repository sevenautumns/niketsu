use std::hash::Hash;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::keyboard::KeyCode;
use iced::theme::{Button as ButtonTheme, Rule as RuleTheme};
use iced::widget::button::{Appearance as ButtonAp, StyleSheet as ButtonSS};
use iced::widget::rule::{Appearance as RuleAp, FillMode, StyleSheet as RuleSS};
use iced::widget::{Button, Column, Rule, Scrollable};
use iced::{Alignment, Element, Length, Renderer, Size, Theme, Vector};
use iced_native::widget::Tree;
use iced_native::{layout, Widget};
use log::*;
use uuid::Uuid;

use crate::window::MainMessage;

pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub enum PlaylistWidgetMessage {
    FileDoubleClick(File),
    FileDelete(File),
    // MoveIndicator(Option<usize>),
    FileMove(File, usize),
    FileInteraction(Option<File>, Interaction),
}

impl From<PlaylistWidgetMessage> for MainMessage {
    fn from(msg: PlaylistWidgetMessage) -> Self {
        MainMessage::FileTable(msg)
    }
}

pub struct PlaylistWidget<'a> {
    base: Element<'a, MainMessage>,
    state: &'a PlaylistWidgetState,
}

impl<'a> PlaylistWidget<'a> {
    pub fn new(state: &'a PlaylistWidgetState) -> Self {
        // TODO Add context menu

        let mut file_btns = vec![];
        for f in state.files.iter() {
            let pressed = state
                .selected
                .as_ref()
                .map_or(false, |f_i| f.uuid.eq(&f_i.uuid));
            file_btns.push(
                Button::new(f.name.as_str())
                    .padding(0)
                    .width(Length::Fill)
                    .style(ButtonTheme::Custom(Box::new(FileTheme { pressed })))
                    .into(),
            );
        }

        Self {
            state,
            base: Scrollable::new(Column::with_children(file_btns)).into(),
        }
    }
}

impl<'a> PlaylistWidget<'a> {
    fn closest_index(
        &self,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
    ) -> Option<(usize, iced::Point)> {
        let files = layout.children().next()?.children();
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
        if closest.1 == self.state.files.len() - 1 {
            if let Some(l) = layout.children().next()?.children().last() {
                let top = l.position();
                let mut bottom = top;
                bottom.y += l.bounds().height;
                let dist = bottom.distance(cursor_position);
                if dist < closest.0 {
                    closest = (dist, self.state.files.len(), bottom)
                }
            }
        }
        // If no index was found return None
        if closest.0.is_infinite() {
            return None;
        }
        // If we are below or above the selected file,
        // dont send an index if we handle files within the playlist
        if let Interaction::Pressing(_) = self.state.interaction {
            if let Some(sele) = &self.state.selected {
                if let Some(clos) = self.state.files.get(closest.1.saturating_sub(1)) {
                    if clos.uuid.eq(&sele.uuid) {
                        return None;
                    }
                }
                if let Some(clos) = self.state.files.get(closest.1) {
                    if clos.uuid.eq(&sele.uuid) {
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
    ) -> Option<File> {
        let files = self
            .state
            .files
            .iter()
            .zip(layout.children().next()?.children());
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
            let interaction = Interaction::Pressing(Instant::now());
            shell.publish(
                PlaylistWidgetMessage::FileInteraction(Some(file.clone()), interaction).into(),
            );

            if let Some(prev_file) = &self.state.selected {
                if let Interaction::Released(when) = self.state.interaction {
                    if file.uuid.eq(&prev_file.uuid) && when.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                        shell.publish(PlaylistWidgetMessage::FileDoubleClick(file).into());
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
            Interaction::PressingExternal => {
                let pos = state.cursor_position;
                if let Some((i, _)) = self.closest_index(layout, pos) {
                    if let Some(name) = file.and_then(|f| {
                        f.file_name()
                            .and_then(|f| f.to_str().map(|f| f.to_string()))
                    }) {
                        let file = File {
                            name,
                            uuid: Uuid::new_v4(),
                        };
                        shell.publish(PlaylistWidgetMessage::FileMove(file, i).into());
                    }
                }
            }
            Interaction::Pressing(_) => {
                let pos = state.cursor_position;
                if let Some((i, _)) = self.closest_index(layout, pos) {
                    if let Some(file) = &self.state.selected {
                        shell.publish(PlaylistWidgetMessage::FileMove(file.clone(), i).into())
                    }
                }
                shell.publish(
                    PlaylistWidgetMessage::FileInteraction(
                        self.state.selected.clone(),
                        Interaction::Released(Instant::now()),
                    )
                    .into(),
                )
            }
            Interaction::Released(_) => {
                shell.publish(
                    PlaylistWidgetMessage::FileInteraction(
                        self.state.selected.clone(),
                        Interaction::None,
                    )
                    .into(),
                );
            }
            Interaction::None => (),
        }

        // if let Some(i) = self.state.insert_hint {
        //     if let Some((f, _)) = &self.state.last_press {
        //         shell.publish(PlaylistWidgetMessage::FileMove(f.clone(), i).into())
        //     }
        // }
    }

    // fn moved(
    //     &self,
    //     layout: iced_native::Layout<'_>,
    //     cursor_position: iced_native::Point,
    //     shell: &mut iced_native::Shell<'_, MainMessage>,
    // ) {
    //     if !self.state.pressing {
    //         return;
    //     }

    //     let selected = match &self
    //         .state
    //         .last_press
    //         .as_ref()
    //         .and_then(|(f, _)| self.state.file_index(f))
    //     {
    //         Some(selected) => *selected,
    //         None => return,
    //     };

    //     let mut closest = self.closest_index(layout, cursor_position);
    //     if let Some(c) = closest {
    //         if c == selected || c == selected + 1 {
    //             closest = None
    //         }
    //     }

    //     if closest != self.state.insert_hint {
    //         shell.publish(PlaylistWidgetMessage::MoveIndicator(closest).into());
    //     }
    // }

    fn deleted(&self, shell: &mut iced_native::Shell<'_, MainMessage>) {
        if let Some(f) = &self.state.selected {
            shell.publish(PlaylistWidgetMessage::FileDelete(f.clone()).into())
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
        self.base.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor_position,
            viewport,
        )
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

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: iced_native::Layout<'_>,
        _renderer: &Renderer,
    ) -> Option<iced_native::overlay::Element<'b, MainMessage, Renderer>> {
        if !self.state.interaction.is_press() {
            return None;
        }
        let inner_state = state.state.downcast_ref::<InnerState>();
        if let Some((_, pos)) = self.closest_index(layout, inner_state.cursor_position) {
            return Some(iced::overlay::Element::new(
                layout.position(),
                Box::new(InsertHint::new(pos)),
            ));
        }
        None
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
        let mut status = iced_native::event::Status::Ignored;
        let mut inner_state = state.state.downcast_mut::<InnerState>();

        // Workaround for if we touch the overlay
        // trace!("{cursor_position:?}")
        if cursor_position != iced::Point::new(-1.0, -1.0) {
            inner_state.cursor_position = cursor_position;
        }

        match &event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key_code,
                modifiers,
            }) => {
                status = iced_native::event::Status::Captured;
                // TODO arrow keys
                // TODO rename lastpressed to selected
                if modifiers.is_empty() && *key_code == KeyCode::Delete {
                    self.deleted(shell)
                }
            }
            iced::Event::Mouse(event) => {
                status = iced_native::event::Status::Captured;
                match event {
                    // iced::mouse::Event::CursorMoved { position } => {
                    //     inner_state.cursor_position = cursor_position;
                    //     // self.moved(layout, *position, shell)
                    // }
                    iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                        self.pressed(layout, cursor_position, shell)
                    }
                    iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                        self.released(None, inner_state, layout, shell)
                    }
                    _ => {}
                }
            }
            iced::Event::Touch(t) => {
                status = iced_native::event::Status::Captured;
                match t {
                    iced::touch::Event::FingerPressed { id: _, position } => {
                        self.pressed(layout, *position, shell)
                    }
                    // iced::touch::Event::FingerMoved { id: _, position } => {
                    //     inner_state.cursor_position = cursor_position;
                    //     // self.moved(layout, *position, shell)
                    // }
                    iced::touch::Event::FingerLifted { id: _, position: _ } => {
                        self.released(None, inner_state, layout, shell)
                    }
                    _ => {}
                }
            }
            iced::Event::Window(event) => {
                //TODO file hover and file dropped
                match event {
                    iced::window::Event::FileHovered(_) => {
                        if !self.state.interaction.is_press_extern() {
                            shell.publish(
                                PlaylistWidgetMessage::FileInteraction(
                                    self.state.selected.clone(),
                                    Interaction::PressingExternal,
                                )
                                .into(),
                            )
                        }
                    }
                    iced::window::Event::FileDropped(file) => {
                        self.released(Some(file.clone()), inner_state, layout, shell)
                    }
                    iced::window::Event::FilesHoveredLeft => shell.publish(
                        PlaylistWidgetMessage::FileInteraction(
                            self.state.selected.clone(),
                            Interaction::None,
                        )
                        .into(),
                    ),
                    _ => {}
                }
            }
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

        match status {
            iced::event::Status::Ignored => inner_status,
            iced::event::Status::Captured => status,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct InnerState {
    cursor_position: iced::Point,
}

#[derive(Debug, Default)]
pub struct PlaylistWidgetState {
    files: Vec<File>,
    selected: Option<File>,
    // last_press: Option<(File, Instant)>,
    // pressing: bool,
    interaction: Interaction,
}

#[derive(Debug, Clone, Default)]
pub enum Interaction {
    PressingExternal,
    Pressing(Instant),
    Released(Instant),
    #[default]
    None,
}

impl Interaction {
    pub fn is_press_extern(&self) -> bool {
        matches!(self, Interaction::PressingExternal)
    }
    pub fn is_press(&self) -> bool {
        matches!(self, Interaction::Pressing(_))
    }
    pub fn is_released(&self) -> bool {
        matches!(self, Interaction::Released(_))
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Interaction::None)
    }
}

impl PlaylistWidgetState {
    // pub fn file_press(&mut self, file: File) {
    //     self.last_press = Some((file, Instant::now()));
    //     self.pressing = true;
    // }

    pub fn move_file(&mut self, file: File, index: usize) -> Vec<File> {
        // TODO reuse for insert of a dropped file at index
        let mut index = index;
        if let Some(i) = self.file_index(&file) {
            if index > i {
                index -= 1
            }
            self.files.remove(i);
            self.files.insert(index, file)
        }

        self.files.clone()
    }

    pub fn file_interaction(&mut self, file: Option<File>, interaction: Interaction) {
        self.selected = file;
        self.interaction = interaction;
    }

    pub fn delete_file(&mut self, file: File) -> Vec<File> {
        // TODO on delete move selected file to file above (or below if non are above)
        if let Some(i) = self.file_index(&file) {
            self.files.remove(i);
        }
        self.files.clone()
    }

    pub fn insert_file(&mut self, file: String, index: usize) -> Vec<File> {
        todo!()
    }

    // pub fn mouse_release(&mut self) {
    //     self.pressing = false;
    // }

    pub fn replace_files(&mut self, files: Vec<String>) {
        // TODO take uuids from old Vec
        let mut files = files;
        self.files = files
            .drain(..)
            .map(|f| File {
                name: f,
                uuid: Uuid::new_v4(),
            })
            .collect();
        // TODO If last pressed file does not exist anymore do something better with the interaction
        // if let Some((file, _)) = &self.last_press {
        //     if !self.files.iter().any(|f| file.uuid.eq(&f.uuid)) {
        //         self.last_press = None;
        //         self.pressing = false;
        //     }
        // }
        self.interaction = Interaction::None;
    }

    pub fn file_index(&self, file: &File) -> Option<usize> {
        for (i, f) in self.files.iter().enumerate() {
            if f.uuid.eq(&file.uuid) {
                return Some(i);
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub uuid: Uuid,
}

impl Hash for File {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uuid.hash(state)
    }
}

impl<'a> From<PlaylistWidget<'a>> for Element<'a, MainMessage> {
    fn from(table: PlaylistWidget<'a>) -> Self {
        Self::new(table)
    }
}

pub struct FileTheme {
    pressed: bool,
}

impl ButtonSS for FileTheme {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> ButtonAp {
        let background = match self.pressed {
            true => Some(iced::Background::Color(style.palette().primary)),
            false => None,
        };
        ButtonAp {
            shadow_offset: Vector::ZERO,
            border_radius: 0.0,
            border_width: 0.0,
            background,
            text_color: style.palette().text,
            ..ButtonAp::default()
        }
    }

    fn hovered(&self, style: &Self::Style) -> ButtonAp {
        self.active(style)
    }

    fn pressed(&self, style: &Self::Style) -> ButtonAp {
        self.active(style)
    }

    fn disabled(&self, style: &Self::Style) -> ButtonAp {
        self.active(style)
    }
}

pub struct FileRuleTheme {
    visible: bool,
}

impl RuleSS for FileRuleTheme {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> RuleAp {
        let mut color = iced::Color::TRANSPARENT;
        if self.visible {
            color = style.palette().text;
        }
        RuleAp {
            color,
            width: 1,
            radius: 0.0,
            fill_mode: FillMode::Full,
        }
    }
}

pub struct InsertHint {
    rule: Rule<Renderer>,
    pos: iced::Point,
}

impl Default for InsertHint {
    fn default() -> Self {
        Self {
            rule: Rule::horizontal(1)
                .style(RuleTheme::Custom(Box::new(FileRuleTheme { visible: true }))),
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
}

impl iced_native::overlay::Overlay<MainMessage, Renderer> for InsertHint {
    fn layout(
        &self,
        renderer: &Renderer,
        bounds: iced::Size,
        _position: iced::Point,
    ) -> iced_native::layout::Node {
        let limits = iced_native::layout::Limits::new(Size::ZERO, bounds)
            .width(Length::Fill)
            .height(1);

        let mut node = <iced::widget::Rule<Renderer> as Widget<MainMessage, Renderer>>::layout(
            &self.rule, renderer, &limits,
        );
        node.move_to(self.pos);

        node
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &<Renderer as iced_native::Renderer>::Theme,
        style: &iced_native::renderer::Style,
        layout: iced_native::Layout<'_>,
        cursor_position: iced::Point,
    ) {
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
