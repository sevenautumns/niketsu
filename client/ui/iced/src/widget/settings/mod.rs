use iced::widget::{
    row, Button, Checkbox, Column, Container, Row, Scrollable, Space, Text, TextInput,
};
use iced::{Alignment, Element, Length, Renderer, Theme};
use niketsu_core::config::Config;

use self::message::{
    Abort, Activate, AddPath, ApplyClose, ApplyCloseSave, DeletePath, PasswordInput, PathInput,
    RoomInput, SecureCheckbox, SettingsWidgetMessage, UrlInput, UsernameInput,
};
use super::overlay::ElementOverlayConfig;
use crate::message::Message;
use crate::styling::{ColorButton, FileButton, ResultButton};
use crate::widget::overlay::ElementOverlay;
use crate::TEXT_SIZE;

pub mod message;

const SPACING: u16 = 10;
const MAX_WIDTH: f32 = 600.0;

pub struct SettingsWidget<'a> {
    button: Element<'a, Message>,
    base: Element<'a, Message>,
    state: &'a SettingsWidgetState,
}

impl<'a> SettingsWidget<'a> {
    pub fn new(state: &'a SettingsWidgetState) -> Self {
        let settings_button = Button::new(
            Text::new("Settings")
                .width(Length::Fill)
                .horizontal_alignment(iced::alignment::Horizontal::Center),
        )
        .on_press(SettingsWidgetMessage::from(Activate).into())
        .width(Length::Fill)
        .style(ResultButton::ready());

        Self {
            button: settings_button.into(),
            base: Self::view(state),
            state,
        }
    }

    pub fn view(state: &'a SettingsWidgetState) -> Element<Message> {
        let text_size = *TEXT_SIZE.load_full();

        let file_paths: Vec<_> = state
            .media_dirs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                row!(
                    TextInput::new("Filepath", d)
                        .on_input(move |p| SettingsWidgetMessage::from(PathInput(i, p)).into()),
                    Button::new(
                        Container::new(Text::new("-"))
                            .center_x()
                            .width(Length::Fill)
                    )
                    .style(ColorButton::theme(Theme::Dark.palette().danger))
                    .on_press(SettingsWidgetMessage::from(DeletePath(i)).into())
                    .width(text_size * 2.0),
                )
                .spacing(SPACING)
                .into()
            })
            .collect();

        let column = Column::new()
            .push(
                Row::new()
                    .push(
                        Text::new("Settings")
                            .size(text_size + 25.0)
                            .width(Length::Fill),
                    )
                    .push(
                        Button::new("Close")
                            .on_press(SettingsWidgetMessage::from(Abort).into())
                            .style(ResultButton::not_ready()),
                    )
                    .spacing(SPACING),
            )
            .push(Space::with_height(text_size))
            .push(
                Text::new("General")
                    .size(text_size + 15.0)
                    .width(Length::Fill),
            )
            .push(
                Row::new()
                    .push(
                        Column::new()
                            .push(
                                Button::new("Server Address").style(FileButton::theme(false, true)),
                            )
                            .push(Button::new("Password").style(FileButton::theme(false, true)))
                            .push(Button::new("Username").style(FileButton::theme(false, true)))
                            .push(Button::new("Room").style(FileButton::theme(false, true)))
                            .spacing(SPACING)
                            .width(Length::Shrink),
                    )
                    .push(
                        Column::new()
                            .push(
                                Row::new()
                                    .push(TextInput::new("Server Address", &state.url).on_input(
                                        |u| SettingsWidgetMessage::from(UrlInput(u)).into(),
                                    ))
                                    .push(
                                        Container::new(
                                            Checkbox::new("Secure", state.secure, |b| {
                                                SettingsWidgetMessage::from(SecureCheckbox(b))
                                                    .into()
                                            })
                                            .spacing(SPACING),
                                        )
                                        .center_y()
                                        .height(text_size + 10.0),
                                    )
                                    .spacing(SPACING),
                            )
                            .push(
                                TextInput::new("Password", &state.password)
                                    .on_input(|u| {
                                        SettingsWidgetMessage::from(PasswordInput(u)).into()
                                    })
                                    .password(),
                            )
                            .push(
                                TextInput::new("Username", &state.username).on_input(|u| {
                                    SettingsWidgetMessage::from(UsernameInput(u)).into()
                                }),
                            )
                            .push(
                                TextInput::new("Room", &state.room)
                                    .on_input(|u| SettingsWidgetMessage::from(RoomInput(u)).into()),
                            )
                            .spacing(SPACING)
                            .width(Length::Fill),
                    )
                    .spacing(SPACING),
            )
            .push(Space::with_height(text_size))
            .push(
                Text::new("Directories")
                    .size(text_size + 15.0)
                    .width(Length::Fill),
            )
            .push(
                Column::new()
                    .push(Column::with_children(file_paths).spacing(SPACING))
                    .push(
                        Button::new(
                            Container::new(Text::new("+"))
                                .center_x()
                                .width(Length::Fill),
                        )
                        .on_press(SettingsWidgetMessage::from(AddPath).into())
                        .width(Length::Fill),
                    )
                    .spacing(SPACING),
            )
            .push(Space::with_height(text_size))
            .push(
                Row::new()
                    .push(
                        Button::new(
                            Text::new("Connect")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .width(Length::Fill)
                        .on_press(SettingsWidgetMessage::from(ApplyClose).into()),
                    )
                    .push(
                        Button::new(
                            Text::new("Save & Connect")
                                .width(Length::Fill)
                                .horizontal_alignment(iced::alignment::Horizontal::Center),
                        )
                        .width(Length::Fill)
                        .on_press(SettingsWidgetMessage::from(ApplyCloseSave).into()),
                    )
                    .spacing(SPACING),
            )
            .align_items(Alignment::Center)
            .width(Length::Fill)
            .max_width(MAX_WIDTH)
            .spacing(SPACING)
            .padding(SPACING);

        Container::new(Scrollable::new(
            Container::new(column)
                .padding(10)
                .center_x()
                .width(Length::Fill),
        ))
        .padding(SPACING)
        .max_width(MAX_WIDTH)
        .center_y()
        .into()
    }
}

impl<'a> iced::advanced::Widget<Message, Renderer> for SettingsWidget<'a> {
    fn width(&self) -> iced::Length {
        self.button.as_widget().width()
    }

    fn height(&self) -> iced::Length {
        self.button.as_widget().height()
    }

    fn layout(
        &self,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.button.as_widget().layout(renderer, limits)
    }

    fn draw(
        &self,
        state: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.button.as_widget().draw(
            &state.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &self,
        state: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation<Message>,
    ) {
        self.button
            .as_widget()
            .operate(&mut state.children[0], layout, renderer, operation);
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![
            iced::advanced::widget::Tree::new(&self.button),
            iced::advanced::widget::Tree::new(&self.base),
        ]
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(&[&self.button, &self.base]);
    }

    fn mouse_interaction(
        &self,
        state: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        self.button.as_widget().mouse_interaction(
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
        event: iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) -> iced::event::Status {
        if self.state.active {
            if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            )) = event
            {
                if matches!(cursor, iced::mouse::Cursor::Available(_)) {
                    shell.publish(SettingsWidgetMessage::from(Abort).into());
                }
            }
            if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key_code,
                modifiers: _,
            }) = event
            {
                if key_code == iced::keyboard::KeyCode::Escape {
                    shell.publish(SettingsWidgetMessage::from(Abort).into());
                }
            }
        }

        self.button.as_widget_mut().on_event(
            &mut state.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Renderer>> {
        if self.state.active {
            return Some(iced::advanced::overlay::Element::new(
                layout.position(),
                Box::new(ElementOverlay {
                    tree: &mut state.children[1],
                    content: &mut self.base,
                    config: ElementOverlayConfig {
                        max_width: Some(MAX_WIDTH),
                        ..Default::default()
                    },
                }),
            ));
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct SettingsWidgetState {
    username: String,
    media_dirs: Vec<String>,
    // TODO validate input with url crate ?
    url: String,
    secure: bool,
    room: String,
    password: String,
    auto_login: bool,
    active: bool,
}

impl SettingsWidgetState {
    pub fn new(config: Config) -> Self {
        Self {
            username: config.username,
            media_dirs: config.media_dirs,
            url: config.url,
            room: config.room,
            password: config.password,
            secure: config.secure,
            auto_login: config.auto_login,
            active: false,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
    }
}

impl From<SettingsWidgetState> for Config {
    fn from(value: SettingsWidgetState) -> Self {
        Config {
            username: value.username,
            media_dirs: value.media_dirs,
            url: value.url,
            room: value.room,
            password: value.password,
            secure: value.secure,
            auto_login: value.auto_login,
        }
    }
}

impl<'a> From<SettingsWidget<'a>> for Element<'a, Message> {
    fn from(table: SettingsWidget<'a>) -> Self {
        Self::new(table)
    }
}
