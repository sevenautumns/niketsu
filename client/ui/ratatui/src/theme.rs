use delegate::delegate;
use ratatui::style::{Color, Modifier, Style};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use strum::{Display, EnumIter, EnumString};

pub trait ThemedWidget {
    fn set_state(&mut self, state: ThemeState) {
        self.theme().set_state(state);
    }
    fn set_theme(&mut self, theme: Theme) {
        self.theme().set_theme(theme);
    }
    fn theme(&mut self) -> &mut ThemeWrapper;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
}

//TODO: check if these themes are approximately correct
impl Theme {
    pub const fn new(bg: Color, fg: Color, accent: Color, hbg: Color, hfg: Color) -> Self {
        Self {
            bg,
            fg,
            accent,
            highlight_bg: hbg,
            highlight_fg: hfg,
        }
    }

    pub const NIKETSU: Self = Self::new(
        Color::Reset,
        Color::White,
        Color::Magenta,
        Color::Reset,
        Color::Cyan,
    );

    pub const DRACULA: Self = Self::new(
        Color::Rgb(40, 42, 54),
        Color::Rgb(248, 248, 242),
        Color::Rgb(255, 121, 198),
        Color::Rgb(68, 71, 90),
        Color::Rgb(139, 233, 253),
    );

    pub const GRUVBOX_DARK: Self = Self::new(
        Color::Rgb(29, 32, 33),
        Color::Rgb(235, 219, 178),
        Color::Rgb(250, 189, 47),
        Color::Rgb(50, 48, 47),
        Color::Rgb(131, 165, 152),
    );

    pub const CATPPUCCIN_FRAPPE: Theme = Theme {
        bg: Color::Rgb(48, 52, 70),
        fg: Color::Rgb(198, 208, 245),
        accent: Color::Rgb(202, 158, 230),
        highlight_bg: Color::Rgb(65, 69, 89),
        highlight_fg: Color::Rgb(153, 209, 219),
    };

    pub const TOKYO_NIGHT: Theme = Theme {
        bg: Color::Rgb(26, 27, 38),
        fg: Color::Rgb(192, 202, 245),
        accent: Color::Rgb(122, 162, 247),
        highlight_bg: Color::Rgb(22, 22, 30),
        highlight_fg: Color::Rgb(125, 207, 255),
    };

    pub const NORD: Theme = Theme {
        bg: Color::Rgb(46, 52, 64),
        fg: Color::Rgb(216, 222, 233),
        accent: Color::Rgb(136, 192, 208),
        highlight_bg: Color::Rgb(59, 66, 82),
        highlight_fg: Color::Rgb(143, 188, 187),
    };

    pub const OXOCARBON_DARK: Theme = Theme {
        bg: Color::Rgb(22, 22, 22),
        fg: Color::Rgb(244, 244, 244),
        accent: Color::Rgb(15, 98, 254),
        highlight_bg: Color::Rgb(38, 38, 38),
        highlight_fg: Color::Rgb(130, 207, 255),
    };

    pub const LIGHT: Self = Self::new(
        Color::Rgb(255, 255, 255),
        Color::Rgb(0, 0, 0),
        Color::Rgb(94, 124, 226),
        Color::Rgb(245, 245, 245),
        Color::Rgb(94, 124, 226),
    );

    pub const DARK: Self = Self::new(
        Color::Rgb(32, 34, 37),
        Color::Rgb(230, 230, 230),
        Color::Rgb(94, 124, 226),
        Color::Rgb(42, 44, 47),
        Color::Rgb(94, 124, 226),
    );

    pub const SOLARIZED_LIGHT: Self = Self::new(
        Color::Rgb(253, 246, 227),
        Color::Rgb(101, 123, 131),
        Color::Rgb(42, 161, 152),
        Color::Rgb(238, 232, 213),
        Color::Rgb(38, 139, 210),
    );

    pub const SOLARIZED_DARK: Self = Self::new(
        Color::Rgb(0, 43, 54),
        Color::Rgb(131, 148, 150),
        Color::Rgb(42, 161, 152),
        Color::Rgb(7, 54, 66),
        Color::Rgb(38, 139, 210),
    );

    pub const GRUVBOX_LIGHT: Self = Self::new(
        Color::Rgb(251, 241, 199),
        Color::Rgb(40, 40, 40),
        Color::Rgb(69, 133, 136),
        Color::Rgb(235, 219, 178),
        Color::Rgb(7, 102, 120),
    );

    pub const KANAGAWA_WAVE: Self = Self::new(
        Color::Rgb(31, 31, 40),
        Color::Rgb(220, 215, 186),
        Color::Rgb(126, 156, 216),
        Color::Rgb(34, 50, 73),
        Color::Rgb(127, 180, 202),
    );

    pub const ATSUI: Self = Self::new(
        Color::Rgb(34, 20, 12),
        Color::Rgb(199, 90, 36),
        Color::Rgb(222, 184, 135),
        Color::Rgb(55, 30, 20),
        Color::Rgb(255, 140, 0),
    );

    pub const AUTUMN: Self = Self::new(
        Color::Rgb(52, 34, 23),
        Color::Rgb(136, 99, 65),
        Color::Rgb(190, 165, 130),
        Color::Rgb(80, 55, 40),
        Color::Rgb(180, 130, 90),
    );

    pub fn base(self) -> Style {
        Style::default().bg(self.bg).fg(self.fg)
    }

    pub fn accent(self) -> Style {
        Style::default().bg(self.bg).fg(self.accent)
    }

    pub fn highlight_fg(self) -> Style {
        Style::default().bg(self.bg).fg(self.highlight_fg)
    }

    pub fn highlight(self) -> Style {
        Style::default()
            .bg(self.highlight_bg)
            .fg(self.highlight_fg)
            .add_modifier(Modifier::BOLD)
    }
}

#[derive(
    SerializeDisplay, DeserializeFromStr, EnumIter, EnumString, Clone, Default, Debug, Display,
)]
pub enum ThemeSelection {
    #[default]
    Niketsu,
    Dark,
    Light,
    #[strum(serialize = "Solarized Dark")]
    SolarizedDark,
    #[strum(serialize = "Solarized Light")]
    SolarizedLight,
    Dracula,
    #[strum(serialize = "Gruvbox Dark")]
    GruvboxDark,
    #[strum(serialize = "Gruvbox Light")]
    GruvboxLight,
    #[strum(serialize = "Catpuccin Frappe")]
    CatpuccinFrappe,
    #[strum(serialize = "Tokyo Night")]
    TokyoNight,
    Nord,
    #[strum(serialize = "Oxocarbon Dark")]
    OxocarbonDark,
    #[strum(serialize = "Kanagawa Wave")]
    KanagawaWave,
    Atsui,
    Autumn,
}

impl From<ThemeSelection> for Theme {
    fn from(value: ThemeSelection) -> Self {
        value.theme()
    }
}

impl ThemeSelection {
    pub const fn theme(&self) -> Theme {
        match self {
            ThemeSelection::Dark => Theme::DARK,
            ThemeSelection::Dracula => Theme::DRACULA,
            ThemeSelection::GruvboxDark => Theme::GRUVBOX_DARK,
            ThemeSelection::Niketsu => Theme::NIKETSU,
            ThemeSelection::Light => Theme::LIGHT,
            ThemeSelection::SolarizedDark => Theme::SOLARIZED_DARK,
            ThemeSelection::SolarizedLight => Theme::SOLARIZED_LIGHT,
            ThemeSelection::GruvboxLight => Theme::GRUVBOX_LIGHT,
            ThemeSelection::CatpuccinFrappe => Theme::CATPPUCCIN_FRAPPE,
            ThemeSelection::TokyoNight => Theme::TOKYO_NIGHT,
            ThemeSelection::Nord => Theme::NORD,
            ThemeSelection::OxocarbonDark => Theme::OXOCARBON_DARK,
            ThemeSelection::KanagawaWave => Theme::KANAGAWA_WAVE,
            ThemeSelection::Autumn => Theme::AUTUMN,
            ThemeSelection::Atsui => Theme::ATSUI,
        }
    }

    pub fn next(&self) -> ThemeSelection {
        match self {
            ThemeSelection::Niketsu => ThemeSelection::Light,
            ThemeSelection::Light => ThemeSelection::Dark,
            ThemeSelection::Dark => ThemeSelection::Dracula,
            ThemeSelection::Dracula => ThemeSelection::GruvboxLight,
            ThemeSelection::GruvboxLight => ThemeSelection::GruvboxDark,
            ThemeSelection::GruvboxDark => ThemeSelection::SolarizedLight,
            ThemeSelection::SolarizedLight => ThemeSelection::SolarizedDark,
            ThemeSelection::SolarizedDark => ThemeSelection::CatpuccinFrappe,
            ThemeSelection::CatpuccinFrappe => ThemeSelection::TokyoNight,
            ThemeSelection::TokyoNight => ThemeSelection::Nord,
            ThemeSelection::Nord => ThemeSelection::OxocarbonDark,
            ThemeSelection::OxocarbonDark => ThemeSelection::KanagawaWave,
            ThemeSelection::KanagawaWave => ThemeSelection::Autumn,
            ThemeSelection::Autumn => ThemeSelection::Atsui,
            ThemeSelection::Atsui => ThemeSelection::Niketsu,
        }
    }

    pub fn previous(&self) -> ThemeSelection {
        match self {
            ThemeSelection::Niketsu => ThemeSelection::Atsui,
            ThemeSelection::Light => ThemeSelection::Niketsu,
            ThemeSelection::Dark => ThemeSelection::Light,
            ThemeSelection::Dracula => ThemeSelection::Dark,
            ThemeSelection::GruvboxLight => ThemeSelection::Dracula,
            ThemeSelection::GruvboxDark => ThemeSelection::GruvboxLight,
            ThemeSelection::SolarizedLight => ThemeSelection::GruvboxDark,
            ThemeSelection::SolarizedDark => ThemeSelection::SolarizedLight,
            ThemeSelection::CatpuccinFrappe => ThemeSelection::SolarizedDark,
            ThemeSelection::TokyoNight => ThemeSelection::CatpuccinFrappe,
            ThemeSelection::Nord => ThemeSelection::TokyoNight,
            ThemeSelection::OxocarbonDark => ThemeSelection::Nord,
            ThemeSelection::KanagawaWave => ThemeSelection::OxocarbonDark,
            ThemeSelection::Autumn => ThemeSelection::KanagawaWave,
            ThemeSelection::Atsui => ThemeSelection::Autumn,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub enum ThemeState {
    #[default]
    Unselected,
    Selected,
    Hovered,
}

#[derive(Clone, Default, Debug)]
pub struct ThemeWrapper {
    inner: Theme,
    state: ThemeState,
}

impl ThemeWrapper {
    pub fn new(theme: Theme) -> Self {
        Self {
            inner: theme,
            state: ThemeState::default(),
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.inner = theme;
    }

    pub fn set_state(&mut self, state: ThemeState) {
        self.state = state;
    }

    pub fn style(&self) -> Style {
        match self.state {
            ThemeState::Unselected => self.inner.base(),
            ThemeState::Selected => self.inner.highlight_fg(),
            ThemeState::Hovered => self.inner.accent(),
        }
    }

    pub fn inner(&self) -> Theme {
        self.inner
    }

    delegate! {
        to self.inner {
            pub fn highlight(&self) -> Style;
            pub fn highlight_fg(&self) -> Style;
            pub fn base(&self) -> Style;
            pub fn accent(&self) -> Style;
        }
    }
}
