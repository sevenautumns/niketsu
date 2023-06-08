#![warn(clippy::unwrap_used)]
#![warn(clippy::too_many_lines)]

use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use config::Config;
use iced::Application;
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

    let settings = Config::load_or_default().into();
    MainWindow::run(settings)?;
    Ok(())
}
