use std::path::PathBuf;

use anyhow::{Context, Result};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::cli::PROJECT_DIRS;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// The relay's identity in its protobuf encoding.
    pub keypair: Option<Vec<u8>>,
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keypair: None,
            port: 7766,
        }
    }
}

impl Config {
    fn file_path() -> Result<PathBuf> {
        let dirs = PROJECT_DIRS
            .as_ref()
            .context("Could not determine config dir")?;
        Ok(dirs.config_dir().join("config.toml"))
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
            Self::default()
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

    /// The relay's identity. A config without one gets a new one, so a
    /// saved config keeps the relay's peer id the same across restarts.
    pub fn identity(&mut self) -> Result<Keypair> {
        if let Some(encoded) = &self.keypair {
            return Ok(Keypair::from_protobuf_encoding(encoded)?);
        }
        let keypair = Keypair::generate_ed25519();
        self.keypair = Some(keypair.to_protobuf_encoding()?);
        Ok(keypair)
    }
}
