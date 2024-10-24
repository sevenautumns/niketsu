use iced::advanced::widget::Operation;
use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::widget::{
    button, checkbox, column, pick_list, row, text, text_input, Button, Column, Container,
    Scrollable, Space, Text,
};
use iced::{Element, Length, Renderer, Theme, Vector};
use message::ThemeChange;
use niketsu_core::config::Config;

use self::message::{
    Abort, Activate, AddPath, ApplyClose, ApplyCloseSave, AutoConnectCheckbox, ConnectApplyClose,
    ConnectApplyCloseSave, DeletePath, PasswordInput, PathInput, Reset, RoomInput,
    SettingsWidgetMessage, UsernameInput,
};
use super::overlay::ElementOverlayConfig;
use crate::config::IcedConfig;
use crate::message::Message;
use crate::styling::FileButton;
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
                .align_x(iced::alignment::Horizontal::Center),
        )
        .on_press(SettingsWidgetMessage::from(Activate).into())
        .width(Length::Fill)
        .style(iced::widget::button::success);

        Self {
            button: settings_button.into(),
            base: Self::view(state),
            state,
        }
    }

    pub fn view(state: &'a SettingsWidgetState) -> Element<Message> {
        let text_size = *TEXT_SIZE.load_full();

        let file_paths: Vec<_> = state
            .config
            .media_dirs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                row!(
                    text_input("Filepath", d)
                        .on_input(move |p| SettingsWidgetMessage::from(PathInput(i, p)).into()),
                    button(Container::new("-").center_x(Length::Fill))
                        .style(iced::widget::button::danger)
                        .on_press(SettingsWidgetMessage::from(DeletePath(i)).into())
                        .width(text_size * 2.0),
                )
                .spacing(SPACING)
                .into()
            })
            .collect();

        let column = column![
            row![
                text("Settings").size(text_size + 25.0).width(Length::Fill),
                button("Reset").on_press(SettingsWidgetMessage::from(Reset).into()),
                button("Close")
                    .on_press(SettingsWidgetMessage::from(Abort).into())
                    .style(iced::widget::button::danger),
            ]
            .spacing(SPACING),
            Space::with_height(text_size),
            text("General").size(text_size + 15.0).width(Length::Fill),
            row![
                column![
                    button("Room").style(FileButton::theme(false, true)),
                    button("Password").style(FileButton::theme(false, true)),
                    button("Username").style(FileButton::theme(false, true)),
                    button("Auto Connect").style(FileButton::theme(false, true)),
                ]
                .spacing(SPACING)
                .width(Length::Shrink),
                column![
                    text_input("Room", &state.config.room)
                        .on_input(|u| { SettingsWidgetMessage::from(RoomInput(u.into())).into() }),
                    text_input("Password", &state.config.password)
                        .on_input(|u| { SettingsWidgetMessage::from(PasswordInput(u)).into() })
                        .secure(true),
                    text_input("Username", &state.config.username).on_input(|u| {
                        SettingsWidgetMessage::from(UsernameInput(u.into())).into()
                    },),
                    Container::new(
                        checkbox("", state.config.auto_connect)
                            .on_toggle(|b| {
                                SettingsWidgetMessage::from(AutoConnectCheckbox(b)).into()
                            })
                            .spacing(SPACING),
                    )
                    .center_y(text_size + 15.0),
                ]
                .spacing(SPACING)
                .width(Length::Fill),
            ]
            .spacing(SPACING),
            Space::with_height(text_size),
            row![
                text("Theme").size(text_size + 15.0).width(Length::Fill),
                pick_list(Theme::ALL, Some(state.iced_config.theme.clone()), |theme| {
                    SettingsWidgetMessage::from(ThemeChange(theme)).into()
                },)
            ],
            Space::with_height(text_size),
            text("Directories")
                .size(text_size + 15.0)
                .width(Length::Fill),
            column![
                Column::with_children(file_paths).spacing(SPACING),
                button(Container::new("+").center_x(Length::Fill))
                    .on_press(SettingsWidgetMessage::from(AddPath).into())
                    .width(Length::Fill),
            ]
            .spacing(SPACING),
            Space::with_height(text_size),
            row![
                button(
                    text("Apply")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .width(Length::Fill)
                .on_press(SettingsWidgetMessage::from(ApplyClose).into()),
                button(
                    text("Connect")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .width(Length::Fill)
                .on_press(SettingsWidgetMessage::from(ConnectApplyClose).into()),
            ]
            .spacing(SPACING),
            row![
                button(
                    text("Apply & Save")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .width(Length::Fill)
                .on_press(SettingsWidgetMessage::from(ApplyCloseSave).into()),
                button(
                    text("Connect & Save")
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                )
                .width(Length::Fill)
                .on_press(SettingsWidgetMessage::from(ConnectApplyCloseSave).into()),
            ]
            .spacing(SPACING),
        ]
        .align_x(iced::alignment::Horizontal::Center)
        .width(Length::Fill)
        .max_width(MAX_WIDTH)
        .spacing(SPACING)
        .padding(SPACING);

        Container::new(Scrollable::new(
            Container::new(column).padding(10).center_x(Length::Fill),
        ))
        .padding(SPACING)
        .max_width(MAX_WIDTH)
        .center_y(Length::Shrink) // TODO maybe this needs to be Fill
        .into()
    }
}

impl<'a> iced::advanced::Widget<Message, Theme, Renderer> for SettingsWidget<'a> {
    fn size(&self) -> iced::Size<Length> {
        self.button.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.button
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
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
        operation: &mut dyn Operation,
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
                key: Key::Named(Named::Escape),
                ..
            }) = event
            {
                shell.publish(SettingsWidgetMessage::from(Abort).into());
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
        _layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
        _translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        if self.state.active {
            return Some(iced::advanced::overlay::Element::new(Box::new(
                ElementOverlay {
                    tree: &mut state.children[1],
                    content: &mut self.base,
                    config: ElementOverlayConfig {
                        max_width: Some(MAX_WIDTH),
                        ..Default::default()
                    },
                },
            )));
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct SettingsWidgetState {
    iced_config: IcedConfig,
    config: Config,
    active: bool,
}

impl SettingsWidgetState {
    pub fn new(config: Config, iced_config: IcedConfig) -> Self {
        Self {
            iced_config,
            config,
            active: false,
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn iced_config(&self) -> &IcedConfig {
        &self.iced_config
    }
}

impl<'a> From<SettingsWidget<'a>> for Element<'a, Message> {
    fn from(table: SettingsWidget<'a>) -> Self {
        Self::new(table)
    }
}
