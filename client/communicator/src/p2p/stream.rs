//! The room's messages, on one stream between the host and each member.
//!
//! A client opens the stream once it is linked to the host, and opening it
//! is joining: its first frame is the client's member tag, which the host
//! checks once. After that the client writes its intents and the host the
//! room's states and lines. A stream is ordered and reliable, so each side
//! gets what the other wrote in the order written, or the stream ends.
//!
//! A frame is a length of four bytes and that much cbor.

use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::io;
use std::task::Poll;

use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, StreamExt};
use libp2p::{PeerId, Stream, StreamProtocol};
use libp2p_stream::{Control, IncomingStreams};
use niketsu_core::playlist::Playlist;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::timeout;
use tracing::{debug, error};

use super::member::{MemberTag, Membership};
use crate::messages::NiketsuMessage;
use crate::timing::MESSAGE_REQUEST_TIMEOUT;

pub(super) const PROTOCOL: StreamProtocol = StreamProtocol::new("/niketsu-room/1");

/// The most a frame may take on the wire; neither side writes or reads a
/// larger one. It bounds the room's playlist.
pub(crate) const MESSAGE_SIZE_MAXIMUM: u64 = 1024 * 1024;

fn encode(msg: &impl Serialize) -> io::Result<Vec<u8>> {
    let bytes = cbor4ii::serde::to_vec(Vec::new(), msg).map_err(io::Error::other)?;
    if bytes.len() as u64 > MESSAGE_SIZE_MAXIMUM {
        return Err(io::Error::other("the frame is too large to write"));
    }
    Ok(bytes)
}

pub(super) async fn write_frame(
    io: &mut (impl AsyncWrite + Unpin),
    msg: &impl Serialize,
) -> io::Result<()> {
    let bytes = encode(msg)?;
    io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    io.write_all(&bytes).await?;
    io.flush().await
}

/// The next frame, or why there is none: the stream ended, or what came is
/// no frame.
pub(super) async fn read_frame<T: DeserializeOwned>(
    io: &mut (impl AsyncRead + Unpin),
) -> io::Result<T> {
    let mut len = [0; 4];
    io.read_exact(&mut len).await?;
    let len = u64::from(u32::from_be_bytes(len));
    if len > MESSAGE_SIZE_MAXIMUM {
        return Err(io::Error::other("the frame is too large to read"));
    }
    let mut bytes = vec![0; len as usize];
    io.read_exact(&mut bytes).await?;
    cbor4ii::serde::from_slice(&bytes).map_err(io::Error::other)
}

/// What waits to be written to one stream, in the order sent. A room
/// state, which only the host sends, is a full snapshot, so a newer one
/// takes the place of one still waiting, at the back of the queue; every
/// other message waits for its turn. A room state carries the playlist
/// when it differs from the last one written to this stream, so with the
/// first state of a stream and with the first after each change.
#[derive(Debug, Default)]
struct Writer {
    waiting: VecDeque<NiketsuMessage>,
    /// The last playlist written.
    playlist: Option<Playlist>,
}

impl Writer {
    fn push(&mut self, msg: NiketsuMessage) {
        if matches!(msg, NiketsuMessage::RoomState(_)) {
            self.waiting
                .retain(|waiting| !matches!(waiting, NiketsuMessage::RoomState(_)));
        }
        self.waiting.push_back(msg);
    }

    /// The next message to write.
    fn next(&mut self) -> Option<NiketsuMessage> {
        let mut msg = self.waiting.pop_front()?;
        if let NiketsuMessage::RoomState(state) = &mut msg {
            if state.playlist == self.playlist {
                state.playlist = None;
            } else {
                self.playlist = state.playlist.clone();
            }
        }
        Some(msg)
    }
}

/// Carries one end of an open stream until it ends: every frame read goes
/// to `received`, and what `outgoing` gets is written, each frame within
/// [`MESSAGE_REQUEST_TIMEOUT`]. The stream ends when the other side ended
/// it, when a write fails or takes too long, or when the session let go of
/// its ends of the two channels.
async fn carry(
    stream: Stream,
    mut outgoing: UnboundedReceiver<NiketsuMessage>,
    received: UnboundedSender<NiketsuMessage>,
) {
    let (mut read, mut write) = stream.split();
    let reading = async {
        loop {
            match read_frame(&mut read).await {
                Ok(msg) => {
                    if received.send(msg).is_err() {
                        return;
                    }
                }
                Err(error) => return debug!(%error, "The stream ended"),
            }
        }
    };
    let writing = async {
        let mut writer = Writer::default();
        while let Some(msg) = outgoing.recv().await {
            writer.push(msg);
            loop {
                while let Ok(msg) = outgoing.try_recv() {
                    writer.push(msg);
                }
                let Some(msg) = writer.next() else {
                    break;
                };
                match timeout(MESSAGE_REQUEST_TIMEOUT, write_frame(&mut write, &msg)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => return debug!(%error, "Failed to write to the stream"),
                    Err(_) => return debug!("A write to the stream took too long"),
                }
            }
        }
    };
    tokio::select! {
        _ = reading => {}
        _ = writing => {}
    }
}

/// The session's ends of an open stream. Its task ends when they are
/// dropped.
struct Ends {
    outgoing: UnboundedSender<NiketsuMessage>,
    incoming: UnboundedReceiver<NiketsuMessage>,
}

/// What happened on the room's streams.
#[derive(Debug)]
pub(crate) enum StreamEvent {
    /// The stream with this peer is open: the host admitted a member, or a
    /// client's stream reached its host.
    Opened(PeerId),
    /// No stream with this peer came about: the host turned it down, or a
    /// client could not open one.
    Refused(PeerId),
    Received(PeerId, NiketsuMessage),
    /// The open stream with this peer ended.
    Ended(PeerId),
}

/// The room's streams this peer has an end of: a host one per member, a
/// client the one to its host.
pub(super) struct Streams {
    control: Control,
    membership: Membership,
    /// Where the streams of joining peers arrive; only a host has it.
    arriving: Option<IncomingStreams>,
    /// Streams on their way to being open: a host checks the tag, a client
    /// opens the stream and presents its own.
    opening: FuturesUnordered<BoxFuture<'static, Result<(PeerId, Stream), PeerId>>>,
    open: HashMap<PeerId, Ends>,
}

impl Streams {
    pub(super) fn new(control: Control, membership: Membership) -> Self {
        Self {
            control,
            membership,
            arriving: None,
            opening: FuturesUnordered::new(),
            open: HashMap::new(),
        }
    }

    /// Takes the streams of joining peers from here on, as the host does.
    pub(super) fn accept(&mut self) {
        match self.control.accept(PROTOCOL) {
            Ok(arriving) => self.arriving = Some(arriving),
            Err(error) => error!(%error, "Nobody can join: failed to accept the room's streams"),
        }
    }

    /// Joins the room of the host at `peer`, to which a direct connection
    /// exists.
    pub(super) fn join(&mut self, peer: PeerId) {
        let mut control = self.control.clone();
        let tag = self.membership.own_tag();
        let opened = async move {
            let mut stream = control
                .open_stream(peer, PROTOCOL)
                .await
                .map_err(io::Error::other)?;
            write_frame(&mut stream, &tag).await?;
            Ok::<_, io::Error>(stream)
        };
        self.opening.push(Box::pin(async move {
            match timeout(MESSAGE_REQUEST_TIMEOUT, opened).await {
                Ok(Ok(stream)) => Ok((peer, stream)),
                Ok(Err(error)) => {
                    debug!(%peer, %error, "Failed to open the stream");
                    Err(peer)
                }
                Err(_) => {
                    debug!(%peer, "Opening the stream took too long");
                    Err(peer)
                }
            }
        }));
    }

    /// Admits `peer` once the first frame of its stream is its member tag.
    fn admit(&mut self, peer: PeerId, mut stream: Stream) {
        let membership = self.membership.clone();
        self.opening.push(Box::pin(async move {
            let tag = timeout(
                MESSAGE_REQUEST_TIMEOUT,
                read_frame::<MemberTag>(&mut stream),
            )
            .await;
            match tag {
                Ok(Ok(tag)) if membership.admits(&peer, &tag) => Ok((peer, stream)),
                _ => Err(peer),
            }
        }));
    }

    /// The stream with `peer` is open. It takes the place of an earlier
    /// one with that peer, which ends unheard: a peer joins again with
    /// each session, also while connections of its previous one are open.
    fn opened(&mut self, peer: PeerId, stream: Stream) {
        let (outgoing, to_write) = unbounded_channel();
        let (received, incoming) = unbounded_channel();
        tokio::spawn(carry(stream, to_write, received));
        self.open.insert(peer, Ends { outgoing, incoming });
    }

    /// Queues `msg` on the open stream of every peer that `to` picks.
    pub(super) fn send(&mut self, msg: NiketsuMessage, to: impl Fn(&PeerId) -> bool) {
        for (peer, ends) in &self.open {
            // a stream that ended is heard of in `next`
            if to(peer) {
                ends.outgoing.send(msg.clone()).ok();
            }
        }
    }

    pub(super) async fn next(&mut self) -> StreamEvent {
        loop {
            tokio::select! {
                (peer, stream) = arrived(&mut self.arriving) => self.admit(peer, stream),
                Some(opened) = self.opening.next() => match opened {
                    Ok((peer, stream)) => {
                        self.opened(peer, stream);
                        return StreamEvent::Opened(peer);
                    }
                    Err(peer) => return StreamEvent::Refused(peer),
                },
                (peer, msg) = received(&mut self.open) => match msg {
                    Some(msg) => return StreamEvent::Received(peer, msg),
                    None => {
                        self.open.remove(&peer);
                        return StreamEvent::Ended(peer);
                    }
                },
            }
        }
    }
}

/// The next stream of a joining peer; never without `arriving`.
async fn arrived(arriving: &mut Option<IncomingStreams>) -> (PeerId, Stream) {
    let next = match arriving {
        Some(arriving) => arriving.next().await,
        None => None,
    };
    match next {
        Some(arrived) => arrived,
        None => std::future::pending().await,
    }
}

/// The next message of any open stream, or `None` from one that ended.
async fn received(open: &mut HashMap<PeerId, Ends>) -> (PeerId, Option<NiketsuMessage>) {
    poll_fn(|cx| {
        for (peer, ends) in open.iter_mut() {
            if let Poll::Ready(msg) = ends.incoming.poll_recv(cx) {
                return Poll::Ready((*peer, msg));
            }
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use futures::io::Cursor;
    use niketsu_core::communicator::{Intent, LineKind, LineMsg, RoomStateMsg};

    use super::*;

    fn line(message: &str) -> NiketsuMessage {
        Intent::Chat(message.into()).into()
    }

    fn action(message: &str) -> NiketsuMessage {
        NiketsuMessage::Line(LineMsg {
            actor: "someone".into(),
            message: message.into(),
            kind: LineKind::Action,
        })
    }

    /// The `n`th state the host publishes.
    fn state(n: u64) -> NiketsuMessage {
        NiketsuMessage::RoomState(RoomStateMsg {
            jumps: n,
            ..Default::default()
        })
    }

    /// The `n`th state, with the room's `playlist`th playlist.
    fn with_playlist(n: u64, playlist: u64) -> NiketsuMessage {
        NiketsuMessage::RoomState(RoomStateMsg {
            jumps: n,
            playlist: Some(Playlist::from_iter([format!("ep{playlist}").as_str()])),
            ..Default::default()
        })
    }

    fn written(writer: &mut Writer) -> Vec<NiketsuMessage> {
        std::iter::from_fn(|| writer.next()).collect()
    }

    #[test]
    fn a_waiting_state_gives_way_to_a_newer_one_and_nothing_else_does() {
        let mut writer = Writer::default();
        writer.push(state(1));
        assert_eq!(writer.next(), Some(state(1)), "written before the rest");
        writer.push(state(2));
        writer.push(line("between"));
        writer.push(action("after the line"));
        writer.push(state(3));
        writer.push(state(4));
        assert_eq!(
            written(&mut writer),
            [line("between"), action("after the line"), state(4)],
            "the waiting states give way to the newest, which keeps its place after the \
             lines sent before it"
        );
    }

    #[test]
    fn the_playlist_goes_with_the_first_state_and_with_the_first_after_a_change() {
        let mut writer = Writer::default();
        let mut carried = Vec::new();
        for (n, playlist) in [(1, 1), (2, 1), (3, 1), (4, 4), (5, 4)] {
            writer.push(with_playlist(n, playlist));
            let msg = writer.next().unwrap();
            let NiketsuMessage::RoomState(state) = msg else {
                panic!("not a state: {msg:?}");
            };
            assert_eq!(state.jumps, n);
            carried.push(state.playlist.is_some());
        }
        assert_eq!(carried, [true, false, false, true, false]);
    }

    /// A state that gave way never reached the stream, so the playlist it
    /// carried still goes out with the state that took its place.
    #[test]
    fn a_playlist_that_gave_way_goes_with_the_newer_state() {
        let mut writer = Writer::default();
        writer.push(with_playlist(1, 1));
        writer.push(with_playlist(2, 1));
        let NiketsuMessage::RoomState(state) = writer.next().unwrap() else {
            panic!("not a state");
        };
        assert_eq!((state.jumps, state.playlist.is_some()), (2, true));
        assert_eq!(writer.next(), None);
    }

    #[tokio::test]
    async fn frames_arrive_whole_and_in_order_and_the_end_is_an_error() {
        let sent = [state(1), line("one"), with_playlist(2, 1)];
        let mut wire = Cursor::new(Vec::new());
        for msg in &sent {
            write_frame(&mut wire, msg).await.unwrap();
        }
        wire.set_position(0);
        for msg in &sent {
            let read: NiketsuMessage = read_frame(&mut wire).await.unwrap();
            assert_eq!(read, *msg);
        }
        assert!(read_frame::<NiketsuMessage>(&mut wire).await.is_err());
    }

    #[tokio::test]
    async fn a_frame_over_the_maximum_is_neither_written_nor_read() {
        let large = line(&"a".repeat(MESSAGE_SIZE_MAXIMUM as usize));
        let mut wire = Cursor::new(Vec::new());
        assert!(write_frame(&mut wire, &large).await.is_err());
        assert!(wire.get_ref().is_empty());

        let len = (MESSAGE_SIZE_MAXIMUM as u32 + 1).to_be_bytes();
        let mut wire = Cursor::new(len.to_vec());
        assert!(read_frame::<NiketsuMessage>(&mut wire).await.is_err());
    }
}
