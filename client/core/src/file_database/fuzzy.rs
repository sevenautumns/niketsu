use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use anyhow::Result;
use futures::Future;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;
use tokio::task::JoinHandle;

use super::{FileEntry, FileStore};

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
            let mut scores = store
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
                .unwrap_or_default();
            scores.par_sort_unstable_by_key(|r| -r.score);
            scores
        })
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub fn poll(&mut self) -> Option<Vec<FuzzyResult>> {
        // TODO replace with Waker::noop when
        // https://github.com/rust-lang/rust/issues/98286 lands
        const VTABLE: RawWakerVTable = RawWakerVTable::new(|_| RAW, |_| {}, |_| {}, |_| {});
        const RAW: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(RAW) };
        let mut ctx = Context::from_waker(&waker);
        let poll = Pin::new(&mut self.handle).poll(&mut ctx);
        match poll {
            Poll::Ready(Ok(res)) => Some(res),
            _ => None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fuzzy_search() {
        let query = String::from("vdo2");

        let entry1 = FileEntry::new(String::from("video1"), "".into(), None);
        let entry2 = FileEntry::new(String::from("video2"), "".into(), None);
        let entry3 = FileEntry::new(String::from("video21"), "".into(), None);
        let entry4 = FileEntry::new(String::from("video12"), "".into(), None);
        let entry5 = FileEntry::new(String::from("video101"), "".into(), None);
        let store = FileStore::from_iter([
            entry1,
            entry2.clone(),
            entry3.clone(),
            entry4.clone(),
            entry5,
        ]);

        let results = FuzzySearch::new(query, store).await;

        assert_eq!(results.len(), 3);
        assert!(results.iter().any(|r| r.entry.eq(&entry2)));
        assert!(results.iter().any(|r| r.entry.eq(&entry3)));
        assert!(results.iter().any(|r| r.entry.eq(&entry4)));
        assert!(results[0].score >= results[1].score);
        assert!(results[1].score >= results[2].score);
    }
}
