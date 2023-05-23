#![warn(clippy::unwrap_used)]
#![warn(clippy::too_many_lines)]

use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use config::Config;
use iced::{Application, Settings};
use iced_window::MainWindow;
use log::*;
use once_cell::sync::Lazy;

pub mod client;
pub mod config;
pub mod iced_window;
pub mod media_player;
pub mod messages;
pub mod playlist;
pub mod rooms;
pub mod styling;
pub mod user;
pub mod video;

pub static TEXT_SIZE: Lazy<ArcSwap<f32>> = Lazy::new(|| ArcSwap::new(Arc::new(14.0)));

fn main() -> Result<()> {
    pretty_env_logger::init();

    let maybe_config = Config::load();
    let config = match maybe_config {
        Ok(conf) => conf,
        Err(e) => {
            warn!("No config loaded: {e:?}");
            Default::default()
        }
    };
    TEXT_SIZE.store(Arc::new(config.text_size));
    let mut settings = Settings::with_flags(config);
    settings.default_text_size = *TEXT_SIZE.load_full();
    settings.window.size = (600, 770);
    // settings
    MainWindow::run(settings)?;
    Ok(())
}
