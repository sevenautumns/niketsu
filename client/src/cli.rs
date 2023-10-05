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
}

#[derive(ValueEnum, Debug, Default, Display, Clone, Copy, PartialEq)]
pub enum UI {
    #[default]
    Iced,
    Ratatui,
}
