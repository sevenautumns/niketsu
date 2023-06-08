use clap::{Parser, ValueEnum};
use strum::Display;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(value_enum, short, long)]
    pub ui: UI,
}

#[derive(ValueEnum, Debug, Default, Display, Clone, Copy, PartialEq)]
pub enum UI {
    #[default]
    Iced,
    Ratatui,
}
