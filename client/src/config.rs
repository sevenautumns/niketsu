use std::path::PathBuf;

use anyhow::{bail, Result};
use directories::ProjectDirs;
use iced::{Color, Theme};
use rgb::RGBA8;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub media_dir: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub password: String,

    #[serde(default = "default_text_size")]
    pub text_size: f32,
    #[serde(default = "default_background")]
    pub background_color: RGBA8,
    #[serde(default = "default_text")]
    pub text_color: RGBA8,
    #[serde(default = "default_primary")]
    pub primary_color: RGBA8,
    #[serde(default = "default_success")]
    pub success_color: RGBA8,
    #[serde(default = "default_danger")]
    pub danger_color: RGBA8,
}

const fn default_text_size() -> f32 {
    14.0
}

const fn default_background() -> RGBA8 {
    RGBA8::new(32, 34, 37, 255)
}

const fn default_text() -> RGBA8 {
    RGBA8::new(230, 230, 230, 255)
}

const fn default_primary() -> RGBA8 {
    RGBA8::new(94, 124, 226, 255)
}

const fn default_success() -> RGBA8 {
    RGBA8::new(18, 102, 79, 255)
}

const fn default_danger() -> RGBA8 {
    RGBA8::new(195, 66, 63, 255)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            username: Default::default(),
            media_dir: Default::default(),
            url: Default::default(),
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
            background: Self::color(self.background_color),
            text: Self::color(self.text_color),
            primary: Self::color(self.primary_color),
            success: Self::color(self.success_color),
            danger: Self::color(self.danger_color),
        })
    }

    fn color(rgba: RGBA8) -> Color {
        Color {
            r: rgba.r as f32 / 255.0,
            g: rgba.g as f32 / 255.0,
            b: rgba.b as f32 / 255.0,
            a: rgba.a as f32 / 255.0,
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
