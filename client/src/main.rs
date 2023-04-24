use anyhow::Result;
use config::Config;
use iced::{Application, Settings};
use log::*;
use window::MainWindow;

pub mod config;
pub mod file_table;
pub mod fs;
pub mod messages;
pub mod mpv;
pub mod player;
pub mod styling;
pub mod user;
pub mod video;
pub mod window;
pub mod ws;

pub const TEXT_SIZE: f32 = 14f32;

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
    let mut settings = Settings::with_flags(config);
    settings.default_text_size = TEXT_SIZE;
    // settings
    MainWindow::run(settings)?;
    Ok(())
}
