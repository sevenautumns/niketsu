use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use arcstr::ArcStr;
use async_trait::async_trait;
use moka::future::Cache;
use niketsu_core::video_server::{ChunkRequest, ServerOnline, VideoServerEvent, VideoServerTrait};
use nom::{IResult, Parser};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;
use tracing::{debug, trace, warn};

const CHUNK_SIZE: u64 = 512_000;
const TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RETRY: usize = 3;
const CACHE_ENTRIES: u64 = 100;

#[derive(Debug, Default)]
pub struct VideoServer {
    server: Option<TcpServerHandle>,
}

#[async_trait]
impl VideoServerTrait for VideoServer {
    fn stop_server(&mut self) {
        self.server.take();
    }

    fn start_server(&mut self, file_name: ArcStr, file_size: u64) {
        self.stop_server();
        let tcp_server = TcpServer::new(file_name, file_size);
        let handle = tcp_server.server_loop();
        self.server = Some(handle);
    }

    fn insert_chunk(&mut self, file_name: &str, start: u64, bytes: Vec<u8>) {
        let Some(handle) = &self.server else {
            return;
        };
        if !handle.file_name.eq(file_name) {
            return;
        }
        let cache = handle.cache.clone();
        tokio::spawn(async move { cache.insert(start, bytes).await });
    }

    fn addr(&self) -> Option<SocketAddr> {
        self.server.as_ref().and_then(|h| h.addr)
    }

    async fn event(&mut self) -> VideoServerEvent {
        let Some(handle) = self.server.as_mut() else {
            std::future::pending().await
        };
        handle.event().await
    }
}

#[derive(Debug)]
struct VideoCache {
    sender: UnboundedSender<RangeInclusive<u64>>,
    cache: Arc<Cache<u64, Vec<u8>>>,
    notify: Arc<Notify>,
}

impl VideoCache {
    fn new(sender: UnboundedSender<RangeInclusive<u64>>) -> Self {
        let cache = Arc::new(Cache::new(CACHE_ENTRIES));
        let notify = Arc::new(Notify::new());
        Self {
            sender,
            cache,
            notify,
        }
    }

    async fn insert(&self, start: u64, bytes: Vec<u8>) {
        self.cache.insert(start, bytes).await;
        self.notify.notify_waiters();
    }

    /// if some chunk containing <start> is already in the cache, refresh it, so it does not get deleted to soon
    /// also if <start> is not the start of the containing chunk, create a new resized chunk with <start> as the beginning
    async fn refresh_chunk(&self, start: u64) -> bool {
        if self.cache.get(&start).await.is_some() {
            return true;
        }
        for (s, bytes) in self.cache.iter() {
            if (*s..(*s + bytes.len() as u64)).contains(&start) {
                trace!(wanted_start = start, chunk_start = %s, "Refreshed chunk");
                self.cache
                    .insert(start, bytes[((start - *s) as usize)..].to_vec())
                    .await;
                return true;
            }
        }
        false
    }

    /// requests a chunk if it is not in the cache yet
    async fn request_chunk(&self, start: u64, end: u64) -> Result<()> {
        if start > end || self.refresh_chunk(start).await {
            return Ok(());
        }
        let end = end.min(start + CHUNK_SIZE);
        self.sender.send(start..=end).map_err(anyhow::Error::from)
    }

    /// wait for a chunk starting at <start> to arrive in the cache
    async fn wait_for_chunk(&self, start: u64) -> Option<Vec<u8>> {
        loop {
            if let Some(bytes) = self.cache.get(&start).await {
                return Some(bytes);
            }
            self.notify.notified().await;
        }
    }

    /// obtain a chunk, starting at <start>, preferably ending at <end>
    async fn obtain_chunk(&self, start: u64, end: u64) -> Result<Vec<u8>> {
        for i in 0..=MAX_RETRY {
            let chunk = tokio::time::timeout(TIMEOUT, self.wait_for_chunk(start)).await;
            if let Some(chunk) = chunk.ok().flatten() {
                return Ok(chunk);
            }
            if i != MAX_RETRY {
                warn!(start, end, retry = i + 1, "Re-Requesting chunk");
                self.request_chunk(start, end).await?;
            }
        }
        bail!("Exeeded Retry Limit")
    }
}

#[derive(Debug)]
struct TcpServerHandle {
    _terminator: Sender<()>,
    addr_rx: Receiver<SocketAddr>,
    addr: Option<SocketAddr>,
    file_name: ArcStr,
    cache: Arc<VideoCache>,
    req_rx: UnboundedReceiver<RangeInclusive<u64>>,
}

impl TcpServerHandle {
    async fn event(&mut self) -> VideoServerEvent {
        tokio::select! {
            Some(addr) = self.addr_rx.recv() => {
                self.addr = Some(addr);
                ServerOnline {
                    file_name: self.file_name.clone(),
                    addr,
                }.into()
            },
            Some(req) = self.req_rx.recv() => {
                ChunkRequest {
                    file_name: self.file_name.clone(),
                    start: *req.start(),
                    length: req.end() - req.start() + 1
                }.into()
            }
        }
    }
}

struct TcpServer {
    file_name: ArcStr,
    file_size: u64,
    cache: Arc<VideoCache>,
    receiver: UnboundedReceiver<RangeInclusive<u64>>,
}

impl TcpServer {
    fn new(file_name: ArcStr, file_size: u64) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cache = Arc::new(VideoCache::new(sender));
        Self {
            file_name,
            file_size,
            cache,
            receiver,
        }
    }

    async fn get_listener(ports: RangeInclusive<u16>) -> Option<TcpListener> {
        let mut listener = None;

        for port in ports {
            let addr = format!("127.0.0.1:{}", port);
            match TcpListener::bind(&addr).await {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(_) => {}
            }
        }

        listener
    }

    fn server_loop(self) -> TcpServerHandle {
        let (_terminator, mut rx) = tokio::sync::mpsc::channel(1);
        let (addr_tx, addr_rx) = tokio::sync::mpsc::channel(1);
        let cache = self.cache.clone();
        tokio::spawn(async move {
            let listener = TcpServer::get_listener(6600..=6700).await.unwrap();

            let addr = listener.local_addr().unwrap();
            addr_tx.send(addr).await.unwrap();
            debug!(%addr, "opened local video server");
            loop {
                tokio::select! {
                    _ = rx.recv() => {
                        return;
                    }
                    Ok((socket, _)) = listener.accept() => {
                        let provider = self.cache.clone();
                        tokio::task::spawn(Self::handle_connection(self.file_size, socket, provider));
                    }
                }
            }
        });
        TcpServerHandle {
            _terminator,
            addr_rx,
            addr: None,
            cache,
            req_rx: self.receiver,
            file_name: self.file_name,
        }
    }

    async fn handle_connection(
        file_size: u64,
        stream: TcpStream,
        provider: Arc<VideoCache>,
    ) -> Result<()> {
        let (read, write) = tokio::io::split(stream);
        let range = Self::process_request(read).await?;
        Self::handle_sending(file_size, write, range, provider).await
    }

    async fn process_request(read: ReadHalf<TcpStream>) -> Result<Range> {
        let reader = BufReader::new(read);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if line.len() <= 2 {
                break;
            }
            if let Ok((_, bounds)) = parse_range_header(&line) {
                if let [first, ..] = bounds[..] {
                    debug!(?first, "incoming request");
                    return Ok(first);
                }
            }
        }
        Ok(Range::default())
    }

    async fn handle_sending(
        file_size: u64,
        mut write: WriteHalf<TcpStream>,
        range: Range,
        provider: Arc<VideoCache>,
    ) -> Result<()> {
        let mut start = range.start(file_size);
        let end = range.end(file_size);
        let length = range.length(file_size);

        provider.request_chunk(start, end).await?;

        let response = format!(
            "HTTP/1.1 206 OK\r\n\
            Content-Length: {length}\r\n\
            Content-Range: bytes {start}-{end}/{file_size}\r\n\r\n"
        );
        let mut res = write.write_all(response.as_bytes()).await;
        while res.is_ok() && start <= end {
            let chunk = provider.obtain_chunk(start, end).await?;
            start += chunk.len() as u64;
            provider.request_chunk(start, end).await?;
            res = write.write_all(&chunk).await;
            let _ = write.flush().await;
        }
        if let Err(error) = res {
            debug!(?error, "Sending stopped")
        }
        let _ = write.flush().await;
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq, PartialOrd, Ord)]
struct Range {
    start: Option<u64>,
    end: Option<u64>,
}

impl Range {
    fn start(&self, size: u64) -> u64 {
        if let (None, Some(i)) = (self.start, self.end) {
            return size - i;
        }
        self.start.unwrap_or_default()
    }

    fn end(&self, size: u64) -> u64 {
        if let (None, Some(_)) = (self.start, self.end) {
            return size - 1;
        }
        self.end.unwrap_or(u64::MAX).min(size - 1)
    }

    fn length(&self, size: u64) -> u64 {
        self.end(size) - self.start(size) + 1
    }
}

impl From<(Option<u64>, Option<u64>)> for Range {
    fn from((left, right): (Option<u64>, Option<u64>)) -> Self {
        Self {
            start: left,
            end: right,
        }
    }
}

fn parse_range_header(input: &str) -> IResult<&str, Vec<Range>> {
    use nom::bytes::complete::{tag, tag_no_case};
    use nom::character::complete::space0;
    use nom::multi::separated_list1;
    use nom::sequence::preceded;

    preceded(
        (tag_no_case("Range:"), space0, tag_no_case("bytes=")),
        separated_list1(tag(","), parse_range),
    )
    .parse(input)
}

fn parse_range(input: &str) -> IResult<&str, Range> {
    use nom::bytes::complete::tag;
    use nom::character::complete::u64;
    use nom::combinator::{map, opt};
    use nom::sequence::separated_pair;

    map(separated_pair(opt(u64), tag("-"), opt(u64)), Range::from).parse(input)
}
