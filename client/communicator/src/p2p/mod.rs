use std::pin::Pin;

use anyhow::{Result, bail};
use async_trait::async_trait;
pub(crate) use file_share::ChunkRoutes;
use file_share::{ChunkRead, FileShareRequest, FileShareResponseResult, PendingChunks};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use libp2p::core::ConnectedPoint;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::{ConnectionError, ConnectionId, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, identify, mdns, noise, ping, relay, tcp, yamux,
};
use links::{Closed, Links, Opened};
use niketsu_core::chunk::{ChunkFetch, ProvideRoute};
use niketsu_core::communicator::{IncomingMessage, OutgoingMessage};
use niketsu_core::log_err_msg;
use niketsu_core::room::RoomName;
use stream::{StreamEvent, Streams};
use tokio::spawn;
use tracing::{debug, warn};

use crate::Node;
use crate::timing::{
    DIRECT_CONNECT_TIMEOUT, FILE_REQUEST_TIMEOUT, IDLE_CONNECTION_TIMEOUT, PING_INTERVAL,
    PING_TIMEOUT, QUIC_HANDSHAKE_TIMEOUT, QUIC_IDLE_TIMEOUT,
};

mod auth;
mod connecting;
mod direct_only;
#[cfg(test)]
mod direct_only_swarm_tests;
mod host_relay;

mod client;
mod file_share;
mod host;
mod links;
mod member;
mod stream;

pub(crate) use auth::AuthDenied;
use direct_only::DirectOnly;
use member::{Membership, Tagged};

/// Result of one iteration of the shared event loop in
/// [`Role::run`]. Short-lived stack value, consumed
/// immediately each iteration; boxing the swarm event would add a heap
/// allocation on the hottest path for no real benefit.
#[allow(clippy::large_enum_variant)]
enum LoopEvent {
    Swarm(SwarmEvent<BehaviourEvent>),
    Core(Option<OutgoingMessage>),
    Stream(StreamEvent),
    /// The local player needs a chunk of a file another peer offers.
    FetchChunk(ChunkFetch),
    /// The local video provider read a chunk for another peer.
    ChunkRead(ResponseChannel<FileShareResponseResult>, Option<Vec<u8>>),
    /// The rendezvous with the host took too long.
    RendezvousExpired,
}

/// Why the event loop of a connection ended. The core reconnects after
/// each of them and shows the reason to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The core dropped the connection.
    Dropped,
    /// No direct connection to the host came about.
    HostUnreachable,
    /// The last direct connection to the host closed.
    HostLost,
    /// The host lost the relay, and its room there with it.
    RelayLost,
    /// The event loop's task panicked.
    Crashed,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dropped => "Connection closed",
            Self::HostUnreachable => "Could not establish a direct connection to the host",
            Self::HostLost => "Lost the connection to the host",
            Self::RelayLost => "Lost the connection to the relay",
            Self::Crashed => "The connection crashed",
        })
    }
}

/// Where the room's host is, seen from this peer.
enum HostLink {
    /// This peer hosts the room.
    Local,
    /// Reachable only through the relay circuit, the rendezvous for the hole
    /// punch. The run loop gives up at `deadline`.
    Rendezvous {
        deadline: Pin<Box<tokio::time::Sleep>>,
    },
    /// Linked by a direct connection, on which the host has `addr`.
    Direct { addr: Multiaddr },
}

impl HostLink {
    fn rendezvous() -> Self {
        Self::Rendezvous {
            deadline: Box::pin(tokio::time::sleep(DIRECT_CONNECT_TIMEOUT)),
        }
    }

    /// The host's address without a trailing `/p2p/…`, so
    /// [`Net::host_circuit`] can build on it.
    fn direct(addr: &Multiaddr) -> Self {
        let mut addr = addr.clone();
        if let Some(Protocol::P2p(_)) = addr.iter().last() {
            addr.pop();
        }
        Self::Direct { addr }
    }

    /// Resolves when the rendezvous took too long, otherwise never.
    async fn rendezvous_expired(&mut self) {
        match self {
            Self::Rendezvous { deadline } => deadline.as_mut().await,
            Self::Local | Self::Direct { .. } => std::future::pending().await,
        }
    }
}

/// Connection setup: relay, identify, hole-punching, keepalive, and room auth.
/// `relay_server` is off until this peer turns out to host the room (see
/// [`host_relay`]).
#[derive(NetworkBehaviour)]
pub(crate) struct TransportBehaviour {
    relay_client: relay::client::Behaviour,
    relay_server: Toggle<relay::Behaviour>,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    ping: ping::Behaviour,
    auth: auth::AuthBehaviour,
}

/// The room's messages, on a stream between the host and each member (see
/// [`stream`]).
pub(crate) type MessagingBehaviour = libp2p_stream::Behaviour;

/// P2P file sharing: chunk transfer, and mDNS for the addresses of peers on
/// the local network. Who offers a video is in the room's state.
#[derive(NetworkBehaviour)]
pub(crate) struct FileShareBehaviour {
    request_response:
        request_response::cbor::Behaviour<Tagged<FileShareRequest>, FileShareResponseResult>,
    mdns: Toggle<mdns::tokio::Behaviour>,
}

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    transport: TransportBehaviour,
    messaging: DirectOnly<MessagingBehaviour>,
    file_share: DirectOnly<FileShareBehaviour>,
}

impl Behaviour {
    fn new(key: &Keypair, relay_client: relay::client::Behaviour, mdns: bool) -> Result<Self> {
        let peer_id = key.public().to_peer_id();
        Ok(Behaviour {
            transport: TransportBehaviour {
                relay_client,
                relay_server: None.into(),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/niketsu-identify/1".to_string(),
                    key.public(),
                )),
                dcutr: dcutr::Behaviour::new(peer_id),
                ping: ping::Behaviour::new(
                    ping::Config::new()
                        .with_interval(PING_INTERVAL)
                        .with_timeout(PING_TIMEOUT),
                ),
                auth: auth::AuthBehaviour::new(),
            },
            messaging: DirectOnly::new(MessagingBehaviour::new()),
            file_share: DirectOnly::new(FileShareBehaviour {
                request_response: request_response::cbor::Behaviour::new(
                    [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                    request_response::Config::default().with_request_timeout(FILE_REQUEST_TIMEOUT),
                ),
                mdns: mdns
                    .then(|| mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id))
                    .transpose()?
                    .into(),
            }),
        })
    }
}

/// The role a peer has in the room: its host or a client. The provided
/// methods are the event loop and everything both roles share (the
/// connections, file sharing); a role supplies what it does with the
/// room's streams and the residual swarm events.
#[async_trait]
pub(crate) trait Role: Send {
    fn session(&mut self) -> &mut Session;
    /// Sends what the core asks of this role, and hands back a message
    /// that only the other role sends.
    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Option<OutgoingMessage>;
    /// What happened on the room's streams, see [`StreamEvent`].
    fn on_stream_event(&mut self, event: StreamEvent);
    /// A connection to `peer` opened, and what that means for the link to
    /// it.
    fn on_opened(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        endpoint: ConnectedPoint,
        opened: Opened,
    );
    /// A connection to `peer` closed, and what that means for the link to
    /// it.
    fn on_closed(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        cause: Option<ConnectionError>,
        closed: Closed,
    );
    /// Role-specific swarm events not covered by `dispatch_swarm_event`.
    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>);

    async fn run(&mut self) -> Stop {
        loop {
            if let Some(stop) = self.session().net.stop {
                return stop;
            }
            let next = {
                let Session { net, streams, .. } = self.session();
                tokio::select! {
                    event = net.swarm.select_next_some() => LoopEvent::Swarm(event),
                    msg = net.core_receiver.recv() => LoopEvent::Core(msg),
                    event = streams.next() => LoopEvent::Stream(event),
                    Some(fetch) = net.chunk_fetches.recv() => LoopEvent::FetchChunk(fetch),
                    Some((channel, bytes)) = net.chunk_reads.next() => {
                        LoopEvent::ChunkRead(channel, bytes)
                    }
                    _ = net.host_link.rendezvous_expired() => LoopEvent::RendezvousExpired,
                }
            };
            match next {
                LoopEvent::Swarm(event) => self.dispatch_swarm_event(event),
                LoopEvent::Core(Some(msg)) => {
                    debug!(?msg, "Handling core message");
                    // the core still acts on a role this session does not
                    // have, e.g. before it has seen the `ConnectedMsg` of a
                    // reconnect
                    if let Some(msg) = self.handle_core_message(msg) {
                        debug!(?msg, "Dropping a core message of the other role");
                    }
                }
                LoopEvent::Stream(event) => {
                    debug!(?event, "Handling event from the room's streams");
                    self.on_stream_event(event);
                }
                LoopEvent::FetchChunk(fetch) => self.session().fs_fetch_chunk(fetch),
                LoopEvent::ChunkRead(channel, bytes) => {
                    let res = self.session().fs_chunk_read(channel, bytes);
                    log_err_msg!(res, "Failed to answer chunk request");
                }
                LoopEvent::Core(None) => {
                    debug!("The core dropped the connection");
                    return Stop::Dropped;
                }
                LoopEvent::RendezvousExpired => {
                    warn!("No direct connection to host within timeout");
                    return Stop::HostUnreachable;
                }
            }
        }
    }

    fn dispatch_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        debug!(?event, "Handling event from swarm");
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        match event {
            SwarmEvent::Behaviour(FileShare(F::RequestResponse(e))) => {
                self.session().fs_req_resp_event(e)
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => {
                let links = &mut self.session().links;
                let opened = links.opened(peer_id, connection_id, endpoint.is_relayed());
                self.on_opened(peer_id, connection_id, endpoint, opened);
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                connection_id,
                cause,
                ..
            } => {
                let closed = self.session().links.closed(peer_id, connection_id);
                self.on_closed(peer_id, connection_id, cause, closed);
            }
            other => self.handle_swarm_event(other),
        }
    }
}

/// Shorthands for what the session asks of its swarm's behaviours.
pub(super) trait SwarmExt {
    fn auth(&mut self) -> &mut auth::AuthBehaviour;
    fn send_file_response(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        msg: FileShareResponseResult,
    ) -> Result<()>;
}

impl SwarmExt for Swarm<Behaviour> {
    fn auth(&mut self) -> &mut auth::AuthBehaviour {
        &mut self.behaviour_mut().transport.auth
    }

    fn send_file_response(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        msg: FileShareResponseResult,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().file_share.inner_mut().request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
    }
}

/// The communicator's end of a session, whose event loop runs in a task
/// of its own, as host or as client.
#[derive(Debug)]
pub(crate) struct SessionHandle {
    sender: tokio::sync::mpsc::UnboundedSender<OutgoingMessage>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<IncomingMessage>,
    event_loop: tokio::task::JoinHandle<Stop>,
}

impl SessionHandle {
    pub(crate) async fn new(
        node: Node,
        relay_addr: Multiaddr,
        room: RoomName,
        password: String,
    ) -> Result<SessionHandle> {
        let Node {
            keypair,
            mdns,
            chunk_routes,
        } = node;
        let mut quic_config = libp2p::quic::Config::new(&keypair.clone());
        quic_config.handshake_timeout = QUIC_HANDSHAKE_TIMEOUT;
        quic_config.max_idle_timeout = QUIC_IDLE_TIMEOUT.as_millis() as u32;

        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                libp2p::noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic_config(|_| quic_config)
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_client| Ok(Behaviour::new(key, relay_client, mdns)?))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(IDLE_CONNECTION_TIMEOUT))
            .build();

        debug!(%relay_addr, "Attempting to connect to relay");

        let (core_sender, core_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (message_sender, message_receiver) = tokio::sync::mpsc::unbounded_channel();

        let mut role = connecting::Setup::new(
            swarm,
            relay_addr,
            room,
            password,
            core_receiver,
            message_sender,
            chunk_routes,
        )
        .run()
        .await?;

        Ok(SessionHandle {
            sender: core_sender,
            receiver: message_receiver,
            event_loop: spawn(async move { role.run().await }),
        })
    }

    /// The next message of the room, or why there will be no more. Not to
    /// be called again after that.
    pub(crate) async fn next(&mut self) -> Result<IncomingMessage, Stop> {
        match self.receiver.recv().await {
            Some(msg) => Ok(msg),
            // the event loop held the sender, so it has ended
            None => Err((&mut self.event_loop).await.unwrap_or(Stop::Crashed)),
        }
    }

    pub(crate) fn send(&self, msg: OutgoingMessage) -> Result<()> {
        debug!(?msg, "Sending message");
        Ok(self.sender.send(msg)?)
    }
}

#[cfg(test)]
impl SessionHandle {
    /// A session whose event loop has ended after handing over `messages`.
    pub(crate) fn ended_with(messages: Vec<IncomingMessage>) -> Self {
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        let (message_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        for message in messages {
            message_sender.send(message).unwrap();
        }
        Self {
            sender,
            receiver,
            event_loop: spawn(async { Stop::HostLost }),
        }
    }
}

/// What a peer keeps of its stay in the room, whichever role it has there.
pub(crate) struct Session {
    net: Net,
    /// The connections to every peer, kept by the shared dispatch. A role
    /// hears what each one means for the link to its peer.
    links: Links,
    /// The room's streams: a host's to its members, a client's to its
    /// host.
    streams: Streams,
    pending_chunks: PendingChunks,
}

impl Session {
    fn new(
        mut swarm: Swarm<Behaviour>,
        membership: Membership,
        host: PeerId,
        host_link: HostLink,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        let control = swarm.behaviour_mut().messaging.inner_mut().new_control();
        let streams = Streams::new(control, membership.clone());
        let net = Net {
            swarm,
            membership,
            host,
            host_link,
            stop: None,
            core_receiver,
            message_sender,
            // a reader waits for its chunk before it asks for the next
            chunk_fetches: chunk_routes.fetch.serve(1),
            provide: chunk_routes.provide,
            chunk_reads: FuturesUnordered::new(),
        };
        Self {
            net,
            links: Links::default(),
            streams,
            pending_chunks: PendingChunks::default(),
        }
    }

    /// Hands the core a message. A core that is gone has dropped the
    /// session, and the run loop ends on that by itself.
    pub fn forward(&mut self, msg: impl Into<IncomingMessage>) {
        if self.net.message_sender.send(msg.into()).is_err() {
            debug!("The core is gone; dropping a message for it");
        }
    }
}

/// The session's ends of everything it talks through: the swarm, the
/// channels to the core and the chunk routes. Apart from the rest of the
/// session, so the file share can use it while the session holds the share.
pub(crate) struct Net {
    swarm: Swarm<Behaviour>,
    /// Tags what this peer sends and checks what other peers send.
    membership: Membership,
    host: PeerId,
    host_link: HostLink,
    /// Set to end the run loop.
    stop: Option<Stop>,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
    /// The chunks the local player asks for of files other peers offer.
    chunk_fetches: tokio::sync::mpsc::Receiver<ChunkFetch>,
    /// The file this peer offers, and where chunk requests of other peers
    /// for it go to the local video provider.
    provide: ProvideRoute,
    chunk_reads: FuturesUnordered<ChunkRead>,
}

impl Net {
    /// Ends the run loop once the current event is handled. The first
    /// reason given is the one reported.
    pub fn stop(&mut self, why: Stop) {
        self.stop.get_or_insert(why);
    }

    pub fn send_file_request(&mut self, peer: &PeerId, msg: FileShareRequest) -> OutboundRequestId {
        let msg = self.membership.tag(msg);
        let file_share = self.swarm.behaviour_mut().file_share.inner_mut();
        file_share.request_response.send_request(peer, msg)
    }

    /// The host's relay circuit, `<host addr>/p2p/HOST/p2p-circuit`: where
    /// clients reserve a slot and reach each other for file-share hole
    /// punches. The relay transport rejects a circuit address without the
    /// relay's address (`MissingRelayAddr`), even though it reuses the
    /// existing connection to the host; hence only a client linked to the
    /// host has one.
    pub fn host_circuit(&self) -> Option<Multiaddr> {
        let HostLink::Direct { addr } = &self.host_link else {
            return None;
        };
        Some(
            addr.clone()
                .with(Protocol::P2p(self.host))
                .with(Protocol::P2pCircuit),
        )
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::chunk::SharedFile;
    use niketsu_core::communicator::{Intent, LineKind, LineMsg, RoomStateMsg};
    use niketsu_core::playlist::{Playlist, Video};
    use niketsu_core::user::User;

    use super::stream::MESSAGE_SIZE_MAXIMUM;
    use super::*;
    use crate::messages::NiketsuMessage;

    /// A playlist of `len` episodes with names as long as a release's.
    fn long_playlist(len: usize) -> Playlist {
        let name = |i: usize| {
            format!(
                "[Release Group] A Fairly Long Series Title S{:02}E{:03} - The Episode Title (1080p BluRay x265 10bit).mkv",
                i / 100,
                i % 100
            )
        };
        (0..len)
            .map(name)
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str)
            .collect()
    }

    fn room_state(playlist: Option<Playlist>) -> RoomStateMsg {
        let video: Video = "[Release Group] A Fairly Long Series Title S01E001.mkv".into();
        // every user has the video loaded and offers it
        let user = |i| User {
            name: format!("someone with a long name {i}").into(),
            ready: true,
            offers: Some(SharedFile {
                name: video.as_str().into(),
                size: u64::MAX,
            }),
            loaded: Some(video.clone()),
            handled: u64::MAX,
        };
        RoomStateMsg {
            jumps: u64::MAX,
            playlist,
            video: Some(video.clone()),
            users: (0..10).map(|i| (PeerId::random(), user(i))).collect(),
            ..Default::default()
        }
    }

    /// What a message puts on the wire: its frame's cbor.
    fn wire_size(msg: NiketsuMessage) -> u64 {
        let bytes = cbor4ii::serde::to_vec(Vec::new(), &msg).unwrap();
        bytes.len() as u64
    }

    /// Every room message reaches its peer with a playlist of 5,000
    /// episodes, and the heartbeat, which goes out twice a second, stays
    /// small whatever the playlist.
    #[test]
    fn every_room_message_fits_with_a_playlist_of_5000_episodes() {
        let playlist = long_playlist(5_000);
        let actor = "someone with a long name";
        let heartbeat = NiketsuMessage::RoomState(room_state(None));
        assert!(wire_size(heartbeat) < 4 * 1024);
        let messages = [
            NiketsuMessage::RoomState(room_state(Some(playlist.clone()))),
            Intent::Playlist(playlist).into(),
            Intent::Chat("a line of chat ".repeat(100)).into(),
            NiketsuMessage::Line(LineMsg {
                actor: actor.into(),
                message: format!("{actor} changed playlist"),
                kind: LineKind::Action,
            }),
            NiketsuMessage::Line(LineMsg {
                actor: actor.into(),
                message: "a line of chat ".repeat(100),
                kind: LineKind::Chat {
                    author: PeerId::random(),
                },
            }),
        ];
        for msg in messages {
            assert!(wire_size(msg) < MESSAGE_SIZE_MAXIMUM);
        }
    }
}
