use std::fs::File;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use directories::ProjectDirs;
use once_cell::sync::{Lazy, OnceCell};
use strum::Display;
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("de", "autumnal", "niketsu-relay"));

static FILE_GUARD: OnceCell<WorkerGuard> = OnceCell::new();

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Set the log level
    #[arg(short = 't', long, default_value_t = LogLevel::default())]
    pub log_level: LogLevel,
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

impl From<LogLevel> for Option<Level> {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => None,
            LogLevel::Error => Some(Level::ERROR),
            LogLevel::Warn => Some(Level::WARN),
            LogLevel::Info => Some(Level::INFO),
            LogLevel::Debug => Some(Level::DEBUG),
            LogLevel::Trace => Some(Level::TRACE),
        }
    }
}

pub fn setup_logger(term_filter: Option<Level>) -> Result<()> {
    let terminal_filter = Targets::new().with_target("niketsu", term_filter);
    let terminal = tracing_subscriber::fmt::Layer::default().with_filter(terminal_filter);
    let tracer = tracing_subscriber::registry().with(terminal);

    let mut log_file = PROJECT_DIRS
        .as_ref()
        .context("Could not get log folder")?
        .cache_dir()
        .to_path_buf();
    std::fs::create_dir_all(log_file.clone())?;
    log_file.push("niketsu-relay.log");
    let appender = File::create(log_file)?;
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(appender);
    FILE_GUARD.set(guard).ok();
    let file_filter = Targets::new().with_target("niketsu", Level::TRACE);
    let file = tracing_subscriber::fmt::Layer::default()
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_writer(non_blocking_appender)
        .with_filter(file_filter);
    tracer.with(file).init();
    Ok(())
}
