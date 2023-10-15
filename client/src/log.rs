use std::fs::File;

use anyhow::Result;
use directories::ProjectDirs;
use log::LevelFilter;
use simplelog::{
    CombinedLogger, Config as LogConfig, ConfigBuilder, SharedLogger, TermLogger, WriteLogger,
};

use crate::cli::LogLevel;

pub fn setup_logger(term_filter: LevelFilter) -> Result<()> {
    let logger = vec![setup_file_logger(), setup_terminal_logger(term_filter)]
        .into_iter()
        .flatten()
        .collect();
    CombinedLogger::init(logger).map_err(anyhow::Error::from)
}

fn setup_config() -> LogConfig {
    ConfigBuilder::new()
        .add_filter_allow("niketsu".to_string())
        .build()
}

fn setup_terminal_logger(filter: LevelFilter) -> Option<Box<dyn SharedLogger>> {
    if let LevelFilter::Off = filter {
        return None;
    }
    Some(TermLogger::new(
        filter,
        setup_config(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    ))
}

fn setup_file_logger() -> Option<Box<dyn SharedLogger>> {
    let mut log_file = ProjectDirs::from("de", "autumnal", "niketsu")?
        .cache_dir()
        .to_path_buf();
    std::fs::create_dir_all(log_file.clone()).ok()?;
    log_file.push("niketsu.log");
    Some(WriteLogger::new(
        LevelFilter::Trace,
        setup_config(),
        File::create(log_file).ok()?,
    ))
}

impl From<LogLevel> for LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}
