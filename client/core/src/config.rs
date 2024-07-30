use std::path::PathBuf;

use anyhow::{bail, Result};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::room::RoomName;
use crate::user::UserStatus;
use crate::PROJECT_DIRS;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Config {
    #[serde(default = "whoami::username")]
    // TODO make this into an ArcStr
    pub username: String,
    #[serde(default)]
    pub media_dirs: Vec<String>,
    #[serde(default = "bootstrap_relay", skip_serializing_if = "is_default")]
    pub relay: String,
    #[serde(default)]
    pub room: RoomName,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub auto_connect: bool,
}

// TODO: look up on 89.58.15.23?
fn bootstrap_relay() -> String {
    "/ip4/127.0.0.1/udp/4001/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        .to_string()
}

fn is_default(value: &String) -> bool {
    *value == bootstrap_relay()
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

    pub fn addr(&self) -> String {
        self.relay.clone()
    }

    pub fn load() -> Result<Self> {
        debug!("load config");
        let path = Self::file_path()?;
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            warn!("no config loaded: {e:?}");
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

    pub(crate) fn status(&self, ready: bool) -> UserStatus {
        UserStatus {
            name: self.username.clone(),
            ready,
        }
    }
}
