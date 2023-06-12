use std::path::PathBuf;
use std::task::Poll;

use anyhow::Result;
use futures::stream::FusedStream;
use futures::Stream;
use log::warn;
use tokio::fs::{DirEntry, ReadDir};

#[derive(Debug)]
pub struct FusedReadDir {
    dir: ReadDir,
    ended: bool,
}

impl FusedReadDir {
    pub async fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            dir: tokio::fs::read_dir(path).await?,
            ended: false,
        })
    }
}

impl Stream for FusedReadDir {
    type Item = DirEntry;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let reader = std::pin::Pin::<&mut FusedReadDir>::into_inner(self);
        let entry = reader.dir.poll_next_entry(cx);
        let Poll::Ready(entry) = entry else {
            return Poll::Pending;
        };
        let entry = entry.unwrap_or_else(|e| {
            warn!("{e:?}");
            None
        });
        if entry.is_none() {
            reader.ended = true;
        }
        Poll::Ready(entry)
    }
}

impl FusedStream for FusedReadDir {
    fn is_terminated(&self) -> bool {
        self.ended
    }
}
