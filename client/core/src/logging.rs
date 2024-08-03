use std::fs::File;

use anyhow::{Context, Result};
use chrono::Local;
use once_cell::sync::OnceCell;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::format::{DefaultVisitor, Writer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use crate::ui::{MessageSource, PlayerMessage, PlayerMessageInner};
use crate::PROJECT_DIRS;

static FILE_GUARD: OnceCell<WorkerGuard> = OnceCell::new();

pub fn setup_logger(
    term_filter: Option<Level>,
    chat_filter: Option<Level>,
) -> Result<Option<ChatLogger>> {
    let (logger, chat_logger) = ChatLoggerSender::new();
    let terminal_filter = Targets::new().with_target("niketsu", term_filter);
    let terminal = tracing_subscriber::fmt::Layer::default().with_filter(terminal_filter);
    let tracer = tracing_subscriber::registry().with(terminal);

    let chat_filter = Targets::new().with_target("niketsu", chat_filter);
    let chat = logger.with_filter(chat_filter);
    let tracer = tracer.with(chat);

    let mut log_file = PROJECT_DIRS
        .as_ref()
        .context("Could not get log folder")?
        .cache_dir()
        .to_path_buf();
    std::fs::create_dir_all(log_file.clone())?;
    log_file.push("niketsu.log");
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

    Ok(Some(chat_logger))
}

impl<S: Subscriber> Layer<S> for ChatLoggerSender {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut message = format!("[{}] ", event.metadata().level());
        let writer = Writer::new(&mut message);
        let mut visitor = DefaultVisitor::new(writer, false);
        event.record(&mut visitor);

        let _ = self.tx.try_send(
            PlayerMessageInner {
                message,
                source: MessageSource::Internal,
                level: (*event.metadata().level()).into(),
                timestamp: Local::now(),
            }
            .into(),
        );
    }
}

#[derive(Debug)]
pub struct ChatLogger {
    rx: Option<Receiver<PlayerMessage>>,
}

impl ChatLogger {
    pub async fn recv(&mut self) -> PlayerMessage {
        if let Some(rx) = &mut self.rx {
            if let Some(message) = rx.recv().await {
                return message;
            }
        }
        futures::future::pending().await
    }
}

#[derive(Debug)]
pub struct ChatLoggerSender {
    tx: Sender<PlayerMessage>,
}

impl ChatLoggerSender {
    pub fn new() -> (Self, ChatLogger) {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let logger = ChatLogger { rx: Some(rx) };
        let sender = Self { tx };
        (sender, logger)
    }
}
