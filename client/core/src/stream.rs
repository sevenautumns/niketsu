//! Reads a file another peer offers the way a player reads a file: as
//! bytes at an offset. The player reads on a thread of its own and waits
//! there for each chunk, so everything here blocks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arcstr::ArcStr;
use multiaddr::PeerId;
use tokio::sync::{Notify, oneshot};
use tracing::warn;

use crate::chunk::{CHUNK_SIZE, ChunkFetch, ChunkRequest, FetchRoute, SharedFile};

/// How often a chunk is asked for before the read fails. Nothing else on
/// the way from the player to the provider asks again.
const ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// The read was cancelled, which says nothing about the stream.
    Cancelled,
    /// A chunk could not be had. The stream is of no use anymore.
    Failed,
}

/// Where a [`Reader`] gets the bytes of its file from.
pub trait Chunks {
    /// The `len` bytes from `start`, or fewer at the end of the file.
    fn chunk(&mut self, start: u64, len: u64) -> Result<Vec<u8>, ReadError>;
}

/// A file read through its chunks. It holds the chunk it last read from,
/// which serves every read that falls into it.
#[derive(Debug)]
pub struct Reader<C = Fetcher> {
    size: u64,
    offset: u64,
    /// Where in the file `chunk` begins.
    start: u64,
    chunk: Vec<u8>,
    chunks: C,
}

impl<C: Chunks> Reader<C> {
    pub fn new(size: u64, chunks: C) -> Self {
        Self {
            size,
            offset: 0,
            start: 0,
            chunk: Vec::new(),
            chunks,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// The next read begins at `offset`.
    pub fn seek(&mut self, offset: u64) {
        self.offset = offset;
    }

    /// Reads into `buf` what the chunk at the offset holds of it, and
    /// moves the offset on by that. Returns 0 at the end of the file.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, ReadError> {
        if self.offset >= self.size {
            return Ok(0);
        }
        let held = self.start..self.start + self.chunk.len() as u64;
        if !held.contains(&self.offset) {
            self.chunk = self.fetch()?;
            self.start = self.offset;
        }
        let from = (self.offset - self.start) as usize;
        let len = buf.len().min(self.chunk.len() - from);
        buf[..len].copy_from_slice(&self.chunk[from..from + len]);
        self.offset += len as u64;
        Ok(len)
    }

    /// The chunk at the offset.
    fn fetch(&mut self) -> Result<Vec<u8>, ReadError> {
        let len = CHUNK_SIZE.min(self.size - self.offset);
        for attempt in 1..=ATTEMPTS {
            match self.chunks.chunk(self.offset, len) {
                Ok(chunk) if !chunk.is_empty() => return Ok(chunk),
                Ok(_) => warn!(
                    offset = self.offset,
                    attempt, "the file ends before its size"
                ),
                Err(ReadError::Failed) => warn!(offset = self.offset, attempt, "no chunk came"),
                Err(ReadError::Cancelled) => return Err(ReadError::Cancelled),
            }
        }
        Err(ReadError::Failed)
    }
}

impl Reader {
    /// A reader of `file` as `provider` offers it, whose chunks come over
    /// `route`. `cancel` ends the wait for a chunk.
    pub fn of(file: &SharedFile, provider: PeerId, route: FetchRoute, cancel: Arc<Cancel>) -> Self {
        let chunks = Fetcher {
            route,
            provider,
            file_name: file.name.clone(),
            cancel,
        };
        Self::new(file.size, chunks)
    }
}

/// Gets chunks from the peer that offers the file, one request at a time.
#[derive(Debug)]
pub struct Fetcher {
    route: FetchRoute,
    provider: PeerId,
    file_name: ArcStr,
    cancel: Arc<Cancel>,
}

impl Chunks for Fetcher {
    /// Waits for the chunk on the calling thread, which must not be one of
    /// an async runtime. The wait has no timeout of its own: it ends when
    /// the communicator answers the request or drops it, or when the read
    /// is cancelled.
    fn chunk(&mut self, start: u64, len: u64) -> Result<Vec<u8>, ReadError> {
        let (reply, bytes) = oneshot::channel();
        let fetch = ChunkFetch {
            provider: self.provider,
            request: ChunkRequest {
                file_name: self.file_name.clone(),
                start,
                len,
                reply,
            },
        };
        let sender = self.route.sender().ok_or(ReadError::Failed)?;
        let fetched = async {
            sender.send(fetch).await.ok()?;
            bytes.await.ok()
        };
        futures::executor::block_on(async {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => Err(ReadError::Cancelled),
                bytes = fetched => bytes.ok_or(ReadError::Failed),
            }
        })
    }
}

/// Ends the reads of a stream from another thread, the one waiting and
/// every later one.
#[derive(Debug, Default)]
pub struct Cancel {
    cancelled: AtomicBool,
    wake: Notify,
}

impl Cancel {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    async fn cancelled(&self) {
        while !self.cancelled.load(Ordering::Acquire) {
            self.wake.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in memory that counts what it is asked for, and fails the
    /// requests it is told to.
    struct Memory {
        content: Vec<u8>,
        asked: Vec<(u64, u64)>,
        failures: usize,
    }

    impl Chunks for &mut Memory {
        fn chunk(&mut self, start: u64, len: u64) -> Result<Vec<u8>, ReadError> {
            self.asked.push((start, len));
            if self.failures > 0 {
                self.failures -= 1;
                return Err(ReadError::Failed);
            }
            let end = self.content.len().min((start + len) as usize);
            Ok(self.content[start as usize..end].to_vec())
        }
    }

    fn memory(len: u64) -> Memory {
        Memory {
            content: (0..len).map(|i| (i % 251) as u8).collect(),
            asked: Vec::new(),
            failures: 0,
        }
    }

    /// Reads with a buffer of `step` bytes until the end of the file.
    fn read_on(reader: &mut Reader<&mut Memory>, step: usize) -> Vec<u8> {
        let mut read = Vec::new();
        let mut buf = vec![0; step];
        loop {
            match reader.read(&mut buf).unwrap() {
                0 => return read,
                len => read.extend_from_slice(&buf[..len]),
            }
        }
    }

    #[test]
    fn a_file_is_read_chunk_by_chunk_to_its_end() {
        let mut file = memory(2 * CHUNK_SIZE + 1_000);
        let mut reader = Reader::new(file.content.len() as u64, &mut file);
        let read = read_on(&mut reader, 100_000);
        assert!(read == file.content);
        let chunks = [
            (0, CHUNK_SIZE),
            (CHUNK_SIZE, CHUNK_SIZE),
            (2 * CHUNK_SIZE, 1_000),
        ];
        assert_eq!(file.asked, chunks);
    }

    #[test]
    fn a_read_is_as_long_as_the_chunk_has_bytes_for_it() {
        let mut file = memory(1_000);
        let mut reader = Reader::new(1_000, &mut file);
        let mut buf = [0; 600];
        assert_eq!(reader.read(&mut buf), Ok(600));
        assert_eq!(reader.read(&mut buf), Ok(400));
        assert_eq!(reader.read(&mut [0; 10]), Ok(0));
        assert_eq!(buf[..400], file.content[600..]);
    }

    #[test]
    fn a_seek_inside_the_chunk_asks_for_nothing() {
        let mut file = memory(CHUNK_SIZE + 1_000);
        let mut reader = Reader::new(file.content.len() as u64, &mut file);
        let mut buf = [0; 100];
        reader.read(&mut buf).unwrap();
        reader.seek(5_000);
        reader.read(&mut buf).unwrap();
        assert_eq!(buf[..], file.content[5_000..5_100]);
        assert_eq!(file.asked.len(), 1);
    }

    #[test]
    fn a_seek_outside_the_chunk_reads_from_there() {
        let mut file = memory(3 * CHUNK_SIZE);
        let mut reader = Reader::new(file.content.len() as u64, &mut file);
        let mut buf = [0; 100];
        reader.read(&mut buf).unwrap();
        let far = 2 * CHUNK_SIZE + 700;
        reader.seek(far);
        reader.read(&mut buf).unwrap();
        let from = far as usize;
        assert_eq!(buf[..], file.content[from..from + 100]);
        assert_eq!(file.asked, [(0, CHUNK_SIZE), (far, CHUNK_SIZE - 700)]);
    }

    #[test]
    fn a_seek_to_the_end_reads_nothing() {
        let mut file = memory(1_000);
        let mut reader = Reader::new(1_000, &mut file);
        reader.seek(1_000);
        assert_eq!(reader.read(&mut [0; 10]), Ok(0));
        assert!(file.asked.is_empty());
    }

    #[test]
    fn a_chunk_that_does_not_come_is_asked_for_again() {
        let mut file = memory(1_000);
        file.failures = ATTEMPTS - 1;
        let mut reader = Reader::new(1_000, &mut file);
        assert_eq!(reader.read(&mut [0; 10]), Ok(10));
        assert_eq!(file.asked.len(), ATTEMPTS);
    }

    #[test]
    fn a_chunk_that_never_comes_fails_the_read() {
        let mut file = memory(1_000);
        file.failures = usize::MAX;
        let mut reader = Reader::new(1_000, &mut file);
        assert_eq!(reader.read(&mut [0; 10]), Err(ReadError::Failed));
        assert_eq!(file.asked.len(), ATTEMPTS);
    }

    #[test]
    fn a_file_shorter_than_offered_fails_the_read() {
        let mut file = memory(1_000);
        let mut reader = Reader::new(2_000, &mut file);
        assert_eq!(read_partially(&mut reader), 1_000);
        assert_eq!(reader.read(&mut [0; 10]), Err(ReadError::Failed));
    }

    /// Reads until the first error and returns how much came before it.
    fn read_partially(reader: &mut Reader<&mut Memory>) -> usize {
        let mut buf = [0; 300];
        let mut read = 0;
        while let Ok(len) = reader.read(&mut buf) {
            read += len;
        }
        read
    }

    /// A read that waits for a chunk nobody sends ends when it is
    /// cancelled, and so does every read after it.
    #[test]
    fn a_cancelled_read_returns() {
        let route = FetchRoute::default();
        let mut requests = route.serve(1);
        let cancel = Arc::new(Cancel::default());
        let file = SharedFile {
            name: "ep1".into(),
            size: 1_000,
        };
        let provider = PeerId::from_bytes(&[0x00, 4, 1, 1, 1, 1]).unwrap();
        let mut reader = Reader::of(&file, provider, route, cancel.clone());

        let reading = std::thread::spawn(move || {
            let first = reader.read(&mut [0; 10]);
            (first, reader.read(&mut [0; 10]))
        });
        let unanswered = requests.blocking_recv().unwrap();
        assert_eq!(unanswered.provider, provider);
        cancel.cancel();
        let cancelled = Err(ReadError::Cancelled);
        assert_eq!(reading.join().unwrap(), (cancelled, cancelled));
    }

    #[test]
    fn a_route_nobody_serves_fails_the_read() {
        let file = SharedFile {
            name: "ep1".into(),
            size: 1_000,
        };
        let provider = PeerId::from_bytes(&[0x00, 4, 1, 1, 1, 1]).unwrap();
        let cancel = Arc::new(Cancel::default());
        let mut reader = Reader::of(&file, provider, FetchRoute::default(), cancel);
        assert_eq!(reader.read(&mut [0; 10]), Err(ReadError::Failed));
    }
}
