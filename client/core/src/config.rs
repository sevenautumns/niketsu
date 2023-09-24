use std::path::PathBuf;

use anyhow::{bail, Result};
use directories::ProjectDirs;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use url::Url;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
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
}

pub const fn default_text_size() -> f32 {
    14.0
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

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            warn!("No config loaded: {e:?}");
            Default::default()
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::file_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(path, toml::to_string(self)?)?)
    }
}
