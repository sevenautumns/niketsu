use std::path::PathBuf;

use crate::theme::ThemeSelection;
use anyhow::{Result, bail};
use directories::ProjectDirs;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_with::{FromInto, serde_as};
use tracing::{debug, warn};

pub static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("de", "autumnal", "niketsu"));

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct RatatuiConfig {
    #[serde_as(as = "FromInto<ThemeSelection>")]
    #[serde(default = "default_theme")]
    pub theme_selection: ThemeSelection,
}

fn default_theme() -> ThemeSelection {
    ThemeSelection::Niketsu
}

impl From<ThemeSelection> for RatatuiConfig {
    fn from(value: ThemeSelection) -> Self {
        Self {
            theme_selection: value,
        }
    }
}

impl From<RatatuiConfig> for ThemeSelection {
    fn from(cfg: RatatuiConfig) -> Self {
        cfg.theme_selection
    }
}

impl RatatuiConfig {
    fn file_path() -> Result<PathBuf> {
        let path = PROJECT_DIRS.as_ref().map(|p| p.config_dir().to_path_buf());
        match path {
            Some(mut path) => {
                path.push("ratatui.toml");
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
