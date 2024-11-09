use iced::widget::rule::FillMode;
use iced::{Border, Color, Shadow, Theme};
use niketsu_core::ui::MessageLevel;

pub struct ContainerBorder;

impl ContainerBorder {
    pub fn theme(theme: &Theme) -> iced::widget::container::Style {
        iced::widget::container::Style {
            border: Border {
                color: theme.palette().text,
                width: 2.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

pub struct FileButton;

impl FileButton {
    pub fn theme(
        pressed: bool,
        available: bool,
    ) -> impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style {
        move |theme, _status| {
            let primary = theme.extended_palette().primary;
            let danger = theme.extended_palette().danger;

            let (background, text) = match (pressed, available) {
                (true, _) => (Some(theme.palette().primary.into()), primary.base.text),
                (_, false) => (Some(theme.palette().danger.into()), danger.base.text),
                _ => (None, theme.palette().text),
            };
            iced::widget::button::Style {
                shadow: Shadow::default(),
                border: Border::default(),
                background,
                text_color: text,
            }
        }
    }
}

pub struct FileRuleTheme;

impl FileRuleTheme {
    pub fn theme(theme: &Theme) -> iced::widget::rule::Style {
        iced::widget::rule::Style {
            color: theme.palette().text,
            width: 1,
            radius: 0.0.into(),
            fill_mode: FillMode::Full,
        }
    }
}

pub struct FileProgressBar;

impl FileProgressBar {
    pub fn theme(finished: bool) -> impl Fn(&Theme) -> iced::widget::progress_bar::Style {
        move |theme| {
            let default = iced::widget::progress_bar::primary(theme);
            let bar = match finished {
                true => iced::Background::Color(theme.palette().success),
                false => iced::Background::Color(theme.palette().primary),
            };
            iced::widget::progress_bar::Style {
                bar,
                background: theme.palette().text.into(),
                border: Border {
                    radius: 5.0.into(),
                    ..default.border
                },
            }
        }
    }
}

pub struct MessageColor;

impl MessageColor {
    pub fn theme(level: MessageLevel) -> impl Fn(&Theme) -> iced::widget::text::Style {
        move |theme| {
            let color = match level {
                MessageLevel::Normal => theme.palette().text,
                MessageLevel::Success => theme.palette().success,
                MessageLevel::Error => theme.palette().danger,
                MessageLevel::Warn => WARN_COLOR,
                MessageLevel::Debug => DEBUG_COLOR,
                MessageLevel::Trace => TRACE_COLOR,
            };
            iced::widget::text::Style { color: Some(color) }
        }
    }
}

pub const WARN_COLOR: Color = Color {
    r: 0.92,
    g: 0.80,
    b: 0.55,
    a: 1.0,
};

pub const DEBUG_COLOR: Color = Color {
    r: 0.51,
    g: 0.63,
    b: 0.76,
    a: 1.0,
};

pub const TRACE_COLOR: Color = Color {
    r: 0.71,
    g: 0.56,
    b: 0.68,
    a: 1.0,
};
