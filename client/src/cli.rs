use clap::{Parser, ValueEnum};
use strum::Display;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// UI to use
    #[arg(value_enum, short, long, default_value_t = UI::Iced)]
    pub ui: UI,
    /// Skip the initial refresh of the file database
    #[arg(short, long)]
    pub skip_database_refresh: bool,
    /// Auto-login from config
    #[arg(short, long)]
    pub auto_login: Option<bool>,
    /// Set the terminal log level (Incompatible with ratatui)
    #[arg(short, long, default_value_t = LogLevel::Off)]
    pub log_level_terminal: LogLevel,
}

#[derive(ValueEnum, Debug, Default, Display, Clone, Copy, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum UI {
    #[default]
    Iced,
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
