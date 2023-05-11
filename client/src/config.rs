use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{bail, Result};
use directories::ProjectDirs;
use iced::{Color, Theme};
use palette::rgb::Rgb;
use palette::Srgb;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use url::Url;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "whoami::username")]
    pub username: String,
    #[serde(default)]
    pub media_dirs: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub password: String,

    #[serde(default = "default_text_size")]
    pub text_size: f32,
    #[serde(default = "default_background")]
    #[serde_as(as = "DisplayFromStr")]
    pub background_color: RgbWrap,
    #[serde(default = "default_text")]
    #[serde_as(as = "DisplayFromStr")]
    pub text_color: RgbWrap,
    #[serde(default = "default_primary")]
    #[serde_as(as = "DisplayFromStr")]
    pub primary_color: RgbWrap,
    #[serde(default = "default_success")]
    #[serde_as(as = "DisplayFromStr")]
    pub success_color: RgbWrap,
    #[serde(default = "default_danger")]
    #[serde_as(as = "DisplayFromStr")]
    pub danger_color: RgbWrap,
}

pub const fn default_text_size() -> f32 {
    14.0
}

pub const fn default_background() -> RgbWrap {
    RgbWrap::new(32, 34, 37)
}

pub const fn default_text() -> RgbWrap {
    RgbWrap::new(230, 230, 230)
}

pub const fn default_primary() -> RgbWrap {
    RgbWrap::new(94, 124, 226)
}

pub const fn default_success() -> RgbWrap {
    RgbWrap::new(18, 102, 79)
}

pub const fn default_danger() -> RgbWrap {
    RgbWrap::new(195, 66, 63)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            username: Default::default(),
            media_dirs: Default::default(),
            url: Default::default(),
            secure: Default::default(),
            room: Default::default(),
            password: Default::default(),
            text_size: default_text_size(),
            background_color: default_background(),
            text_color: default_text(),
            primary_color: default_primary(),
            success_color: default_success(),
            danger_color: default_danger(),
        }
    }
}

impl Config {
    fn file_path() -> Result<PathBuf> {
        let path =
            ProjectDirs::from("de", "autumnal", "niketsu").map(|p| p.config_dir().to_path_buf());
        match path {
            Some(mut path) => {
                path.push("config.toml");
                Ok(path)
            }
            None => bail!("Could not determine config dir"),
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(iced::theme::Palette {
            background: self.background_color.into(),
            text: self.text_color.into(),
            primary: self.primary_color.into(),
            success: self.success_color.into(),
            danger: self.danger_color.into(),
        })
    }

    pub fn addr(&self) -> Result<Url> {
        match self.secure {
            true => Ok(Url::parse(&format!("wss://{}", self.url))?),
            false => Ok(Url::parse(&format!("ws://{}", self.url))?),
        }
    }

    pub fn load() -> Result<Self> {
        let path = Self::file_path()?;
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::file_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(path, toml::to_string(self)?)?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RgbWrap(Rgb<Srgb, u8>);

impl Display for RgbWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("#{:X}", self.0))
    }
}

impl FromStr for RgbWrap {
    type Err = palette::rgb::FromHexError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Rgb::from_str(s).map(Self)
    }
}

impl RgbWrap {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(Rgb::new(r, g, b))
    }
}

impl From<RgbWrap> for Color {
    fn from(c: RgbWrap) -> Self {
        Self {
            r: c.0.red as f32 / 255.0,
            g: c.0.green as f32 / 255.0,
            b: c.0.blue as f32 / 255.0,
            a: 1.0,
        }
    }
}
