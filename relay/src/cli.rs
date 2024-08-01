use std::fs::File;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use directories::ProjectDirs;
use log::{debug, LevelFilter};
use once_cell::sync::Lazy;
use simplelog::{
    CombinedLogger, Config as LogConfig, ConfigBuilder, SharedLogger, TermLogger, WriteLogger,
};
use strum::Display;

pub static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("de", "autumnal", "niketsu-relay"));

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Set the log level
    #[arg(short = 't', long, default_value_t = LogLevel::default())]
    pub log_level: LogLevel,
    /// Use ipv6 instead of ipv4
    #[arg(short = 'i', long)]
    pub ipv6: Option<bool>,
    /// Set port to listen on
    #[arg(short = 'p', long)]
    pub port: Option<u16>,
}

#[derive(ValueEnum, Debug, Default, Display, Clone, Copy, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum LogLevel {
    #[default]
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
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

pub fn setup_logger(term_filter: LevelFilter) -> Result<()> {
    let logger = vec![setup_file_logger(), setup_terminal_logger(term_filter)]
        .into_iter()
        .flatten()
        .collect();
    CombinedLogger::init(logger).map_err(anyhow::Error::from)?;
    Ok(())
}

fn setup_terminal_logger(filter: LevelFilter) -> Option<Box<dyn SharedLogger>> {
    if let LevelFilter::Off = filter {
        return None;
    }
    debug!("setup terminal logger");
    Some(TermLogger::new(
        filter,
        setup_config(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    ))
}

fn setup_config() -> LogConfig {
    ConfigBuilder::new()
        .add_filter_allow(String::from("niketsu"))
        .build()
}

fn setup_file_logger() -> Option<Box<dyn SharedLogger>> {
    debug!("setup file logger");
    let mut log_file = PROJECT_DIRS.as_ref()?.cache_dir().to_path_buf();
    std::fs::create_dir_all(log_file.clone()).ok()?;
    log_file.push("niketsu-relay.log");
    Some(WriteLogger::new(
        LevelFilter::Trace,
        setup_config(),
        File::create(log_file).ok()?,
    ))
}
