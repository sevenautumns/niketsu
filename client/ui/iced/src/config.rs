use std::convert::Infallible;
use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{bail, Result};
use directories::ProjectDirs;
use iced::Theme;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DeserializeFromStr, FromInto, SerializeDisplay};
use tracing::{debug, warn};

pub static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("de", "autumnal", "niketsu"));

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct IcedConfig {
    #[serde_as(as = "FromInto<ThemeWrapper>")]
    #[serde(default = "default_theme")]
    pub theme: Theme,
}

fn default_theme() -> Theme {
    Theme::Dark
}

impl IcedConfig {
    fn file_path() -> Result<PathBuf> {
        let path = PROJECT_DIRS.as_ref().map(|p| p.config_dir().to_path_buf());
        match path {
            Some(mut path) => {
                path.push("iced.toml");
                Ok(path)
            }
            None => bail!("Could not determine config dir"),
        }
    }

    pub fn load() -> Result<Self> {
        debug!("load config");
        let path = Self::file_path()?;
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|error| {
            warn!(%error, "no config loaded");
            Default::default()
        })
    }

    pub fn save(&self) -> Result<()> {
        debug!("save config");
        let path = Self::file_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(path, toml::to_string(self)?)?)
    }
}

#[derive(SerializeDisplay, DeserializeFromStr, Debug, Clone, Default)]
pub struct ThemeWrapper(pub Theme);

impl Display for ThemeWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Theme> for ThemeWrapper {
    fn from(value: Theme) -> Self {
        Self(value)
    }
}

impl From<ThemeWrapper> for Theme {
    fn from(value: ThemeWrapper) -> Self {
        value.0
    }
}

impl FromStr for ThemeWrapper {
    type Err = Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Theme::ALL
            .iter()
            .find(|t| t.to_string().eq(s))
            .cloned()
            .map(ThemeWrapper::from)
            .unwrap_or_else(|| default_theme().into()))
    }
}
