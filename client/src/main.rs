use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwapOption;
use config::Config;
use iced::{Application, Settings};
use log::*;
use window::MainWindow;

pub mod config;
pub mod file_table;
pub mod fs;
pub mod messages;
pub mod mpv;
pub mod rooms;
pub mod styling;
pub mod user;
pub mod video;
pub mod window;
pub mod ws;

pub static TEXT_SIZE: ArcSwapOption<f32> = ArcSwapOption::const_empty();

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
    TEXT_SIZE.store(Some(Arc::new(config.text_size)));
    let mut settings = Settings::with_flags(config);
    settings.default_text_size = *TEXT_SIZE.load_full().unwrap();
    // settings
    MainWindow::run(settings)?;
    Ok(())
}
