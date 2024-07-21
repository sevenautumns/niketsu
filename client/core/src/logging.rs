use std::fs::File;

use anyhow::Result;
use chrono::Local;
use log::{debug, LevelFilter, Log};
use simplelog::{
    CombinedLogger, Config as LogConfig, ConfigBuilder, SharedLogger, TermLogger, WriteLogger,
};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::ui::{MessageSource, PlayerMessage, PlayerMessageInner};
use crate::PROJECT_DIRS;

pub fn setup_logger(
    term_filter: LevelFilter,
    chat_filter: LevelFilter,
) -> Result<Option<ChatLogger>> {
    let (logger, chat_logger) = setup_chat_logger(chat_filter);
    let logger = vec![
        setup_file_logger(),
        setup_terminal_logger(term_filter),
        logger,
    ]
    .into_iter()
    .flatten()
    .collect();
    CombinedLogger::init(logger).map_err(anyhow::Error::from)?;
    Ok(chat_logger)
}

fn setup_config() -> LogConfig {
    ConfigBuilder::new()
        .add_filter_allow(String::from("niketsu"))
        .build()
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

fn setup_chat_logger(filter: LevelFilter) -> (Option<Box<dyn SharedLogger>>, Option<ChatLogger>) {
    if let LevelFilter::Off = filter {
        return (None, None);
    }
    let (logger, chat_logger) = ChatLoggerSender::new(filter);
    (Some(logger), Some(chat_logger))
}

fn setup_file_logger() -> Option<Box<dyn SharedLogger>> {
    debug!("setup file logger");
    let mut log_file = PROJECT_DIRS.as_ref()?.cache_dir().to_path_buf();
    std::fs::create_dir_all(log_file.clone()).ok()?;
    log_file.push("niketsu.log");
    Some(WriteLogger::new(
        LevelFilter::Trace,
        setup_config(),
        File::create(log_file).ok()?,
    ))
}

#[derive(Debug)]
pub struct ChatLogger {
    rx: Option<Receiver<PlayerMessage>>,
}

impl ChatLogger {
    pub async fn recv(&mut self) -> Option<PlayerMessage> {
        if let Some(rx) = &mut self.rx {
            return rx.recv().await;
        }
        futures::future::pending().await
    }
}

#[derive(Debug)]
pub struct ChatLoggerSender {
    tx: Sender<PlayerMessage>,
    filter: LevelFilter,
}

impl ChatLoggerSender {
    pub fn new(filter: LevelFilter) -> (Box<Self>, ChatLogger) {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let logger = ChatLogger { rx: Some(rx) };
        let sender = Self { tx, filter };
        (Box::new(sender), logger)
    }
}

impl Log for ChatLoggerSender {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        if metadata.level() > self.filter {
            return false;
        }
        metadata.target().starts_with("niketsu")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = format!("[{}] {}", record.level(), record.args());
        let _ = self.tx.try_send(
            PlayerMessageInner {
                message,
                source: MessageSource::Internal,
                level: record.level().into(),
                timestamp: Local::now(),
            }
            .into(),
        );
    }

    fn flush(&self) {}
}

impl SharedLogger for ChatLoggerSender {
    fn level(&self) -> LevelFilter {
        self.filter
    }

    fn config(&self) -> Option<&LogConfig> {
        None
    }

    fn as_log(self: Box<Self>) -> Box<dyn Log> {
        self
    }
}
