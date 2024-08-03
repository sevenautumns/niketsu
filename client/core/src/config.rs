use std::path::PathBuf;

use anyhow::{bail, Result};
use arcstr::ArcStr;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tracing::{debug, warn};

use crate::room::RoomName;
use crate::user::UserStatus;
use crate::PROJECT_DIRS;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "get_username")]
    pub username: ArcStr,
    #[serde(default)]
    pub media_dirs: Vec<String>,
    #[serde(default = "bootstrap_relay", skip_serializing_if = "is_default_relay")]
    pub relay: String,
    #[serde(default = "bootstrap_port", skip_serializing_if = "is_default_port")]
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub room: RoomName,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub auto_connect: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            relay: bootstrap_relay(),
            port: bootstrap_port(),
            peer_id: Default::default(),
            username: Default::default(),
            media_dirs: Default::default(),
            room: Default::default(),
            password: Default::default(),
            auto_connect: Default::default(),
        }
    }
}

fn get_username() -> ArcStr {
    whoami::username().into()
}

fn bootstrap_relay() -> String {
    "autumnal.de".to_string()
}

fn bootstrap_port() -> u16 {
    7766
}

fn is_default_port(value: &u16) -> bool {
    *value == bootstrap_port()
}

fn is_default_relay(value: &String) -> bool {
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
        if let Some(peer_id) = self.peer_id.clone() {
            format!(
                "/dns/{}/udp/{}/quic-v1/p2p/{}",
                self.relay, self.port, peer_id
            )
        } else {
            format!("/dns/{}/udp/{}/quic-v1", self.relay, self.port)
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

    pub(crate) fn status(&self, ready: bool) -> UserStatus {
        UserStatus {
            name: self.username.clone(),
            ready,
        }
    }
}
