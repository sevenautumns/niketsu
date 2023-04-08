use std::hash::Hash;
use std::time::{Duration, Instant};

use iced::keyboard::KeyCode;
use iced::theme::{Button as ButtonTheme, Rule as RuleTheme};
use iced::widget::button::{Appearance as ButtonAp, StyleSheet as ButtonSS};
use iced::widget::rule::{Appearance as RuleAp, FillMode, StyleSheet as RuleSS};
use iced::widget::{Button, Column, Rule, Scrollable};
use iced::{Element, Length, Renderer, Theme, Vector};
use iced_native::widget::Tree;
use iced_native::Widget;
use log::*;
use uuid::Uuid;

use crate::window::MainMessage;

pub const MAX_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub enum FileTableMessage {
    FilePress(File),
    FileDoubleClick(File),
    FileDelete(File),
    MoveIndicator(Option<usize>),
    FileMove(File, usize),
    MouseRelease,
}

impl From<FileTableMessage> for MainMessage {
    fn from(msg: FileTableMessage) -> Self {
        MainMessage::FileTable(msg)
    }
}

pub struct FileTable<'a> {
    base: Element<'a, MainMessage>,
    state: &'a FileTableState,
}

impl<'a> FileTable<'a> {
    pub fn new(state: &'a FileTableState) -> Self {
        // TODO Add context menu
        let mut file_btns = vec![];
        let hint_index = state.insert_hint.unwrap_or(state.files.len() + 2);
        for (i, f) in state.files.iter().enumerate() {
            file_btns.push(
                Rule::horizontal(1)
                    .style(RuleTheme::Custom(Box::new(FileRuleTheme {
                        visible: i == hint_index,
                    })))
                    .into(),
            );
            let pressed = state
                .last_press
                .as_ref()
                .map_or(false, |(f_i, _)| f.uuid.eq(&f_i.uuid));
            file_btns.push(
                Button::new(f.name.as_str())
                    .padding(0)
                    .width(Length::Fill)
                    .style(ButtonTheme::Custom(Box::new(FileTheme { pressed })))
                    .into(),
            );
        }

        file_btns.push(
            Rule::horizontal(1)
                .style(RuleTheme::Custom(Box::new(FileRuleTheme {
                    visible: state.files.len() == hint_index,
                })))
                .into(),
        );

        Self {
            state,
            base: Scrollable::new(Column::with_children(file_btns)).into(),
        }
    }
}

impl<'a> FileTable<'a> {
    fn closest_rule_index(
        &self,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
    ) -> Option<usize> {
        let rules = layout.children().next()?.children().step_by(2);
        let mut closest = (f32::INFINITY, 0);
        for (i, layout) in rules.enumerate() {
            let dist = layout.position().distance(cursor_position);
            if dist < closest.0 {
                closest = (dist, i)
            }
        }
        if closest.0.is_infinite() {
            return None;
        }
        Some(closest.1)
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
            .zip(layout.children().next()?.children().skip(1).step_by(2));
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
            if let Some(i) = self.state.file_index(&file) {
                shell.publish(FileTableMessage::FilePress(file.clone()).into());

                if let Some((prev_file, when)) = &self.state.last_press {
                    if file.uuid.eq(&prev_file.uuid) && when.elapsed() < MAX_DOUBLE_CLICK_INTERVAL {
                        shell.publish(FileTableMessage::FileDoubleClick(file).into());
                    }
                }
            }
        }
    }

    fn released(&self, shell: &mut iced_native::Shell<'_, MainMessage>) {
        shell.publish(FileTableMessage::MouseRelease.into());

        if let Some(i) = self.state.insert_hint {
            if let Some((f, _)) = &self.state.last_press {
                shell.publish(FileTableMessage::FileMove(f.clone(), i).into())
            }
        }
    }

    fn moved(
        &self,
        layout: iced_native::Layout<'_>,
        cursor_position: iced_native::Point,
        shell: &mut iced_native::Shell<'_, MainMessage>,
    ) {
        if !self.state.pressing {
            return;
        }

        let selected = match &self
            .state
            .last_press
            .as_ref()
            .and_then(|(f, _)| self.state.file_index(f))
        {
            Some(selected) => *selected,
            None => return,
        };

        let mut closest = self.closest_rule_index(layout, cursor_position);
        if let Some(c) = closest {
            if c == selected || c == selected + 1 {
                closest = None
            }
        }

        if closest != self.state.insert_hint {
            shell.publish(FileTableMessage::MoveIndicator(closest).into());
        }
    }

    fn deleted(&self, shell: &mut iced_native::Shell<'_, MainMessage>) {
        if let Some((f, _)) = &self.state.last_press {
            shell.publish(FileTableMessage::FileDelete(f.clone()).into())
        }
    }
}

impl<'a> Widget<MainMessage, Renderer> for FileTable<'a> {
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
        renderer: &Renderer,
    ) -> Option<iced_native::overlay::Element<'b, MainMessage, Renderer>> {
        // TODO move insert hint to overlay
        self.base.as_widget_mut().overlay(
            &mut state.children[0],
            layout.children().next().unwrap(),
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
        let mut status = iced_native::event::Status::Ignored;
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
                    iced::mouse::Event::CursorMoved { position } => {
                        self.moved(layout, *position, shell)
                    }
                    iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) => {
                        self.pressed(layout, cursor_position, shell)
                    }
                    iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                        self.released(shell)
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
                    iced::touch::Event::FingerMoved { id: _, position } => {
                        self.moved(layout, *position, shell)
                    }
                    iced::touch::Event::FingerLifted { id: _, position: _ } => self.released(shell),
                    _ => {}
                }
            }
            iced::Event::Window(event) => {
                //TODO file hover and file dropped
                // todo!()
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

#[derive(Debug, Default)]
pub struct FileTableState {
    files: Vec<File>,
    last_press: Option<(File, Instant)>,
    pressing: bool,
    insert_hint: Option<usize>,
}

impl FileTableState {
    pub fn file_press(&mut self, file: File) {
        self.last_press = Some((file, Instant::now()));
        self.pressing = true;
    }

    pub fn move_file(&mut self, file: File, index: usize) -> Vec<File> {
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

    pub fn mouse_release(&mut self) {
        self.pressing = false;
        self.insert_hint = None;
    }

    pub fn move_indicator(&mut self, indicator: Option<usize>) {
        self.insert_hint = indicator
    }

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
        // If last pressed file does not exist anymore, reset last_press, pressing and insert_hint
        if let Some((file, _)) = &self.last_press {
            if !self.files.iter().any(|f| file.uuid.eq(&f.uuid)) {
                self.last_press = None;
                self.pressing = false;
                self.insert_hint = None;
            }
        }
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

impl<'a> From<FileTable<'a>> for Element<'a, MainMessage> {
    fn from(table: FileTable<'a>) -> Self {
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
