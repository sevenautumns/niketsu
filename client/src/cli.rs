use clap::{Parser, ValueEnum};
use log::LevelFilter;
use strum::Display;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// UI to use
    #[arg(value_enum, short, long, default_value_t = UI::default())]
    pub ui: UI,
    /// Skip the initial refresh of the file database
    #[arg(short, long)]
    pub skip_database_refresh: bool,
    /// Auto-login from config
    #[arg(short, long)]
    pub auto_login: Option<bool>,
    /// Set the terminal log level (Incompatible with ratatui)
    #[arg(short = 't', long, default_value_t = LogLevel::default())]
    pub log_level_terminal: LogLevel,
    /// Set the chat log level
    #[arg(short = 'c', long, default_value_t = LogLevel::default())]
    pub log_level_chat: LogLevel,
}

#[derive(ValueEnum, Debug, Default, Display, Clone, Copy, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum UI {
    #[cfg(feature = "iced")]
    #[cfg_attr(feature = "iced", default)]
    Iced,
    #[cfg(feature = "ratatui")]
    #[cfg_attr(all(feature = "ratatui", not(feature = "iced")), default)]
    Ratatui,
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
