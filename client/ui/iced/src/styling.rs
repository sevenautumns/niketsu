use iced::widget::rule::FillMode;
use iced::{Color, Theme, Vector};
use niketsu_core::ui::MessageLevel;

pub struct ResultButton {
    success: bool,
}

impl ResultButton {
    pub fn not_ready() -> iced::theme::Button {
        iced::theme::Button::Custom(Box::new(Self { success: false }))
    }

    pub fn ready() -> iced::theme::Button {
        iced::theme::Button::Custom(Box::new(Self { success: true }))
    }

    pub fn theme(success: bool) -> iced::theme::Button {
        iced::theme::Button::Custom(Box::new(Self { success }))
    }

    pub fn background(&self, style: &Theme) -> Option<iced::Background> {
        match self.success {
            true => Some(iced::Background::Color(style.palette().success)),
            false => Some(iced::Background::Color(style.palette().danger)),
        }
    }
}

impl iced::widget::button::StyleSheet for ResultButton {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        iced::widget::button::Appearance {
            background: self.background(style),
            ..style.active(&iced::theme::Button::Text)
        }
    }

    fn hovered(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        iced::widget::button::Appearance {
            background: self.background(style),
            ..style.hovered(&iced::theme::Button::Text)
        }
    }

    fn pressed(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        iced::widget::button::Appearance {
            background: self.background(style),
            ..style.pressed(&iced::theme::Button::Text)
        }
    }

    fn disabled(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        iced::widget::button::Appearance {
            background: self.background(style),
            ..style.disabled(&iced::theme::Button::Text)
        }
    }
}

pub struct ContainerBorder;

impl ContainerBorder {
    pub fn basic() -> iced::theme::Container {
        iced::theme::Container::Custom(Box::new(Self))
    }
}

impl iced::widget::container::StyleSheet for ContainerBorder {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            border_color: style.palette().text,
            border_radius: 5.0.into(),
            border_width: 2.0,
            ..Default::default()
        }
    }
}

pub struct FileButton {
    pressed: bool,
    available: bool,
}

impl FileButton {
    pub fn theme(pressed: bool, available: bool) -> iced::theme::Button {
        iced::theme::Button::Custom(Box::new(Self { pressed, available }))
    }
}

impl iced::widget::button::StyleSheet for FileButton {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        let background = match (self.pressed, self.available) {
            (true, _) => Some(iced::Background::Color(style.palette().primary)),
            (_, false) => Some(iced::Background::Color(style.palette().danger)),
            _ => None,
        };
        iced::widget::button::Appearance {
            shadow_offset: Vector::ZERO,
            border_radius: 0.0.into(),
            border_width: 0.0,
            background,
            text_color: style.palette().text,
            ..iced::widget::button::Appearance::default()
        }
    }

    fn hovered(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        self.active(style)
    }

    fn pressed(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        self.active(style)
    }

    fn disabled(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        self.active(style)
    }
}

pub struct FileRuleTheme;

impl FileRuleTheme {
    pub fn theme() -> iced::theme::Rule {
        iced::theme::Rule::Custom(Box::new(Self))
    }
}

impl iced::widget::rule::StyleSheet for FileRuleTheme {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> iced::widget::rule::Appearance {
        iced::widget::rule::Appearance {
            color: style.palette().text,
            width: 1,
            radius: 0.0.into(),
            fill_mode: FillMode::Full,
        }
    }
}

pub struct FileProgressBar {
    finished: bool,
}

impl FileProgressBar {
    pub fn theme(finished: bool) -> iced::theme::ProgressBar {
        iced::theme::ProgressBar::Custom(Box::new(Self { finished }))
    }
}

impl iced::widget::progress_bar::StyleSheet for FileProgressBar {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> iced::widget::progress_bar::Appearance {
        let bar = match self.finished {
            true => iced::Background::Color(style.palette().success),
            false => iced::Background::Color(style.palette().primary),
        };
        iced::widget::progress_bar::Appearance {
            bar,
            background: style.palette().text.into(),
            border_radius: 5.0.into(),
        }
    }
}

pub struct ContainerBackground {
    color: Color,
}

impl iced::widget::container::StyleSheet for ContainerBackground {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            text_color: style.palette().text.into(),
            background: iced::Background::Color(self.color).into(),
            ..Default::default()
        }
    }
}

pub struct ContainerText {
    color: Color,
}

impl iced::widget::container::StyleSheet for ContainerText {
    type Style = Theme;

    fn appearance(&self, _: &Self::Style) -> iced::widget::container::Appearance {
        iced::widget::container::Appearance {
            text_color: Some(self.color),
            ..Default::default()
        }
    }
}

pub struct ColorButton {
    color: Color,
}

impl ColorButton {
    pub fn theme(color: Color) -> iced::theme::Button {
        iced::theme::Button::Custom(Box::new(Self { color }))
    }
}

impl iced::widget::button::StyleSheet for ColorButton {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        iced::widget::button::Appearance {
            background: iced::Background::Color(self.color).into(),
            ..style.active(&iced::theme::Button::Text)
        }
    }
}

pub struct MessageColor {
    level: MessageLevel,
}

impl MessageColor {
    pub fn theme(level: MessageLevel) -> iced::theme::Container {
        iced::theme::Container::Custom(Box::new(Self { level }))
    }
}

impl iced::widget::container::StyleSheet for MessageColor {
    type Style = Theme;

    fn appearance(&self, style: &Self::Style) -> iced::widget::container::Appearance {
        let color = match self.level {
            MessageLevel::Normal => style.palette().text,
            MessageLevel::Success => style.palette().success,
            MessageLevel::Error => style.palette().danger,
            MessageLevel::Warn => WARN_COLOR,
            MessageLevel::Debug => DEBUG_COLOR,
            MessageLevel::Trace => TRACE_COLOR,
        };
        ContainerText { color }.appearance(style)
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
