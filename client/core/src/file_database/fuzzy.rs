use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Poll;

use anyhow::Result;
use futures::Future;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use tokio::task::JoinHandle;

use super::{FileEntry, FileStore};
// use crate::file_database::FileEntry;

#[derive(Debug)]
pub struct FuzzySearch {
    handle: JoinHandle<Vec<FuzzyResult>>,
    stop: Arc<AtomicBool>,
}

impl FuzzySearch {
    pub fn new(query: String, store: FileStore) -> Self {
        let stop = Arc::<AtomicBool>::default();
        let handle = Self::search(query, store, stop.clone());

        Self { handle, stop }
    }

    pub fn search(
        query: String,
        store: FileStore,
        stop: Arc<AtomicBool>,
    ) -> JoinHandle<Vec<FuzzyResult>> {
        tokio::task::spawn_blocking(move || {
            let matcher = SkimMatcherV2::default();
            store
                .par_iter()
                .filter_map(|entry| {
                    if stop.load(Ordering::Relaxed) {
                        return Some(Err(anyhow::anyhow!("search stopped")));
                    }
                    let (score, hits) = matcher.fuzzy_indices(entry.file_name(), &query)?;
                    Some(Ok(FuzzyResult {
                        score,
                        hits,
                        entry: entry.clone(),
                    }))
                })
                .collect::<Result<Vec<_>>>()
                .unwrap_or_default()
        })
    }

    pub fn abort(self) {
        drop(self)
    }
}

impl Future for FuzzySearch {
    type Output = Vec<FuzzyResult>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let handle = unsafe { self.map_unchecked_mut(|s| &mut s.handle) };
        let poll = handle.poll(cx);
        let Poll::Ready(results) = poll else {
            return Poll::Pending;
        };
        let results = results.unwrap_or_default();
        Poll::Ready(results)
    }
}

impl Drop for FuzzySearch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct FuzzyResult {
    pub score: i64,
    pub hits: Vec<usize>,
    pub entry: FileEntry,
}
