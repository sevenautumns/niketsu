use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tracing::{debug, warn};

use crate::cli::PROJECT_DIRS;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub keypair: Option<Vec<u8>>,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    7766
}

impl Config {
    fn file_path() -> Result<PathBuf> {
        let path = PROJECT_DIRS.as_ref().map(|p| p.config_dir().to_path_buf());
        match path {
            Some(mut path) => {
                path.push("config.toml");
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
            Config {
                keypair: None,
                port: default_port(),
            }
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
