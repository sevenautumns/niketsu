use std::pin::Pin;
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
pub(crate) use file_share::ChunkRoutes;
use file_share::{ChunkRead, FileShare, FileShareRequest, FileShareResponseResult};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use libp2p::core::ConnectedPoint;
use libp2p::gossipsub::PublishError;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::swarm::{ConnectionError, ConnectionId, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, dcutr, gossipsub, identify, kad, mdns, noise, ping, relay,
    tcp, yamux,
};
use links::{Closed, Links, Opened};
use niketsu_core::chunk::{ChunkRequest, ChunkRoute};
use niketsu_core::communicator::{IncomingMessage, OutgoingMessage, RoomMessage, UserMessageMsg};
use niketsu_core::log_err_msg;
use niketsu_core::playlist::Video;
use niketsu_core::room::RoomName;
use serde::{Deserialize, Serialize};
use sha256::digest;
use tokio::spawn;
use tracing::{debug, error, trace, warn};

use crate::Node;
use crate::messages::NiketsuMessage;
use crate::timing::{
    DIRECT_CONNECT_TIMEOUT, FILE_REQUEST_TIMEOUT, IDLE_CONNECTION_TIMEOUT, MESSAGE_REQUEST_TIMEOUT,
    PING_INTERVAL, PING_TIMEOUT, QUIC_HANDSHAKE_TIMEOUT, QUIC_IDLE_TIMEOUT,
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
mod order;

use direct_only::DirectOnly;

/// Result of one iteration of the shared event loop in
/// [`Role::run`]. Short-lived stack value, consumed
/// immediately each iteration; boxing the swarm event would add a heap
/// allocation on the hottest path for no real benefit.
#[allow(clippy::large_enum_variant)]
enum LoopEvent {
    Swarm(SwarmEvent<BehaviourEvent>),
    Core(Option<OutgoingMessage>),
    /// The local video server needs a chunk from the provider.
    FetchChunk(ChunkRequest),
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
    /// The relay moved the room to the new host.
    HandedOver,
    HandoverFailed,
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
            Self::HandedOver => "Handed the room over",
            Self::HandoverFailed => "The relay refused to hand the room over",
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

/// Room sync protocol: gossip broadcasts and direct host↔client messages.
#[derive(NetworkBehaviour)]
pub(crate) struct MessagingBehaviour {
    gossipsub: gossipsub::Behaviour,
    request_response: request_response::cbor::Behaviour<MessageRequest, MessageResponse>,
}

/// P2P file sharing: provider discovery (kademlia + mDNS) and chunk transfer.
#[derive(NetworkBehaviour)]
pub(crate) struct FileShareBehaviour {
    request_response: request_response::cbor::Behaviour<FileShareRequest, FileShareResponseResult>,
    kademlia: kad::Behaviour<MemoryStore>,
    mdns: Toggle<mdns::tokio::Behaviour>,
}

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    transport: TransportBehaviour,
    messaging: DirectOnly<MessagingBehaviour>,
    file_share: DirectOnly<FileShareBehaviour>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageRequest(NiketsuMessage);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MessageResponse(StatusResponse);

/// Whether the receiver took a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum StatusResponse {
    Ok,
    Err,
}

/// The role a peer has in the room: its host or a client. The provided
/// methods are the event loop and everything both roles share (gossip
/// broadcasts, direct messages, file sharing); a role supplies its own
/// message handling and the residual swarm events.
#[async_trait]
pub(crate) trait Role: Send {
    fn session(&mut self) -> &mut Session;
    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Result<()>;
    fn handle_swarm_broadcast(&mut self, msg: NiketsuMessage, source: PeerId) -> Result<()>;
    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()>;
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

    /// Request-response events other than `Message` (role-specific logging).
    fn on_msg_req_resp_other(
        &mut self,
        event: request_response::Event<MessageRequest, MessageResponse>,
    ) {
        debug!(
            ?event,
            "Received request response event that is not handled"
        );
    }

    async fn run(&mut self) -> Stop {
        loop {
            if let Some(stop) = self.session().net.stop {
                return stop;
            }
            let next = {
                let session = self.session();
                let can_fetch = session.can_fetch_chunk();
                let net = &mut session.net;
                tokio::select! {
                    event = net.swarm.select_next_some() => LoopEvent::Swarm(event),
                    msg = net.core_receiver.recv() => LoopEvent::Core(msg),
                    Some(request) = net.chunk_fetches.recv(), if can_fetch => {
                        LoopEvent::FetchChunk(request)
                    }
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
                    let res = self.handle_core_message(msg);
                    log_err_msg!(res, "Handling message caused error");
                }
                LoopEvent::FetchChunk(request) => {
                    let res = self.session().fs_fetch_chunk(request);
                    log_err_msg!(res, "Failed to fetch chunk");
                }
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
        use MessagingBehaviourEvent as M;
        match event {
            SwarmEvent::Behaviour(Messaging(M::Gossipsub(e))) => self.on_gossipsub(e),
            SwarmEvent::Behaviour(Messaging(M::RequestResponse(e))) => self.on_msg_req_resp(e),
            SwarmEvent::Behaviour(FileShare(F::RequestResponse(e))) => {
                self.session().fs_req_resp_event(e)
            }
            SwarmEvent::Behaviour(FileShare(F::Kademlia(e))) => self.session().fs_kad_event(e),
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

    fn on_gossipsub(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                message_id,
                message,
                ..
            } => {
                debug!(%message_id, msg = %String::from_utf8_lossy(&message.data),
                    "Received gossipsub message",
                );
                // propagation_source is only the last hop in the gossip mesh;
                // the signed source identifies the actual author.
                let (Some(source), Some(sequence)) = (message.source, message.sequence_number)
                else {
                    warn!(%message_id, "Dropping gossipsub message without signed source");
                    return;
                };
                let msg = match NiketsuMessage::try_from(message.data) {
                    Ok(msg) => msg,
                    Err(error) => return error!(%error, "Failed to read broadcast message"),
                };
                if self.session().newest.is_stale(source, sequence, &msg) {
                    return debug!(?msg, %source, "Dropping a broadcast that a newer one replaced");
                }
                let res = self.handle_swarm_broadcast(msg, source);
                log_err_msg!(res, "Failed to handle broadcast message");
            }
            gossipsub_event => debug!(
                ?gossipsub_event,
                "Received gossipsub event that is not handled"
            ),
        }
    }

    fn on_msg_req_resp(&mut self, event: request_response::Event<MessageRequest, MessageResponse>) {
        match event {
            request_response::Event::Message { peer, message, .. } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    let req = request.0;
                    trace!(?req, "Received request");
                    let res = self.handle_swarm_request(req, channel, peer);
                    log_err_msg!(res, "Failed to handle incoming message");
                }
                request_response::Message::Response { response, .. } => {
                    debug!(?response, %peer, "Received response");
                }
            },
            other => self.on_msg_req_resp_other(other),
        }
    }
}

/// Shorthands for what the session asks of its swarm's behaviours.
pub(super) trait SwarmExt {
    fn gossipsub(&mut self) -> &mut gossipsub::Behaviour;
    fn kademlia(&mut self) -> &mut kad::Behaviour<MemoryStore>;
    fn auth(&mut self) -> &mut auth::AuthBehaviour;
    fn send_request(&mut self, peer_id: &PeerId, msg: NiketsuMessage) -> OutboundRequestId;
    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId;
    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()>;
    fn send_file_response(
        &mut self,
        channel: ResponseChannel<FileShareResponseResult>,
        msg: FileShareResponseResult,
    ) -> Result<()>;
    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()>;
    fn start_providing(&mut self, video: &Video) -> Result<()>;
    fn stop_providing(&mut self, video: &Video);
}

impl SwarmExt for Swarm<Behaviour> {
    fn gossipsub(&mut self) -> &mut gossipsub::Behaviour {
        &mut self.behaviour_mut().messaging.inner_mut().gossipsub
    }

    fn kademlia(&mut self) -> &mut kad::Behaviour<MemoryStore> {
        &mut self.behaviour_mut().file_share.inner_mut().kademlia
    }

    fn auth(&mut self) -> &mut auth::AuthBehaviour {
        &mut self.behaviour_mut().transport.auth
    }

    fn send_request(&mut self, peer_id: &PeerId, msg: NiketsuMessage) -> OutboundRequestId {
        let req_resp = &mut self.behaviour_mut().messaging.inner_mut().request_response;
        req_resp.send_request(peer_id, MessageRequest(msg))
    }

    fn send_file_request(&mut self, peer_id: &PeerId, msg: FileShareRequest) -> OutboundRequestId {
        let req_resp = &mut self.behaviour_mut().file_share.inner_mut().request_response;
        req_resp.send_request(peer_id, msg)
    }

    fn send_message_response(
        &mut self,
        channel: ResponseChannel<MessageResponse>,
        msg: MessageResponse,
    ) -> Result<()> {
        let req_resp = &mut self.behaviour_mut().messaging.inner_mut().request_response;
        let res = req_resp.send_response(channel, msg);

        match res {
            Ok(_) => debug!("Successfully sent response status"),
            Err(e) => bail!("Failed to send response status {e:?}"),
        }
        Ok(())
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

    fn try_broadcast(&mut self, topic: gossipsub::IdentTopic, msg: NiketsuMessage) -> Result<()> {
        let res = self.gossipsub().publish(topic, Vec::<u8>::try_from(msg)?);

        match res {
            Err(PublishError::NoPeersSubscribedToTopic) => {
                debug!("Gossipsub insufficient peers. Publishing when no one is connected")
            }
            Err(err) => return Err(anyhow::Error::from(err)),
            _ => {}
        }
        Ok(())
    }

    fn start_providing(&mut self, video: &Video) -> Result<()> {
        let filename = video.as_str().as_bytes().to_vec();
        let res = self.kademlia().start_providing(filename.clone().into());

        match res {
            Ok(id) => debug!(?filename, ?id, "Successfully started providing file"),
            Err(e) => bail!("Failed to start providing {e:?}"),
        }
        Ok(())
    }

    fn stop_providing(&mut self, video: &Video) {
        let filename = video.as_str().as_bytes().to_vec();
        self.kademlia().stop_providing(&filename.clone().into());
        debug!(?filename, "Stopped providing file");
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
            .with_behaviour(|key, relay_behaviour| {
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .duplicate_cache_time(Duration::from_secs(60))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(anyhow::Error::from)?;

                Ok(Behaviour {
                    transport: TransportBehaviour {
                        relay_client: relay_behaviour,
                        relay_server: None.into(),
                        identify: identify::Behaviour::new(identify::Config::new(
                            "/niketsu-identify/1".to_string(),
                            key.public(),
                        )),
                        dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
                        ping: ping::Behaviour::new(
                            ping::Config::new()
                                .with_interval(PING_INTERVAL)
                                .with_timeout(PING_TIMEOUT),
                        ),
                        auth: auth::AuthBehaviour::new(),
                    },
                    messaging: DirectOnly::new(MessagingBehaviour {
                        gossipsub: gossipsub::Behaviour::new(
                            gossipsub::MessageAuthenticity::Signed(key.clone()),
                            gossipsub_config,
                        )?,
                        request_response: request_response::cbor::Behaviour::new(
                            [(
                                StreamProtocol::new("/niketsu-message/1"),
                                ProtocolSupport::Full,
                            )],
                            request_response::Config::default()
                                .with_request_timeout(MESSAGE_REQUEST_TIMEOUT),
                        ),
                    }),
                    file_share: DirectOnly::new(FileShareBehaviour {
                        request_response: request_response::cbor::Behaviour::new(
                            [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                            request_response::Config::default()
                                .with_request_timeout(FILE_REQUEST_TIMEOUT),
                        ),
                        // Kademlia stays in its default auto mode: it answers
                        // queries only once the swarm confirms an external
                        // address, which only an accepted relay reservation
                        // does here. Without the reservations every node stays
                        // a kad client and provider discovery silently dies.
                        kademlia: kad::Behaviour::new(
                            keypair.public().to_peer_id(),
                            MemoryStore::new(key.public().to_peer_id()),
                        ),
                        mdns: mdns
                            .then(|| {
                                mdns::tokio::Behaviour::new(
                                    mdns::Config::default(),
                                    key.public().to_peer_id(),
                                )
                            })
                            .transpose()?
                            .into(),
                    }),
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(IDLE_CONNECTION_TIMEOUT))
            .build();

        debug!(%relay_addr, "Attempting to connect to relay");

        let topic_hash = digest(format!("{room}|{password}"));
        let topic = gossipsub::IdentTopic::new(topic_hash);

        let (core_sender, core_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (message_sender, message_receiver) = tokio::sync::mpsc::unbounded_channel();

        let mut role = connecting::Setup::new(
            swarm,
            relay_addr,
            room,
            password,
            topic,
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
    file_share: Option<FileShare>,
    /// Answers whether a broadcast or the host's greeting is older than
    /// state already applied.
    newest: order::Newest,
}

impl Session {
    fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        host: PeerId,
        host_link: HostLink,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        let net = Net {
            swarm,
            topic,
            host,
            host_link,
            stop: None,
            core_receiver,
            message_sender,
            chunk_fetches: chunk_routes.fetch.serve(file_share::CHUNK_FETCHES),
            provide: chunk_routes.provide,
            chunk_reads: FuturesUnordered::new(),
        };
        Self {
            net,
            links: Links::default(),
            file_share: None,
            newest: order::Newest::default(),
        }
    }

    /// Routing shared by both roles: room-wide gossip and file sharing.
    pub fn route_room_message(&mut self, msg: RoomMessage) -> Result<()> {
        use RoomMessage::*;
        match msg {
            Start(msg) => self.broadcast(msg.into()),
            Pause(msg) => self.broadcast(msg.into()),
            PlaybackSpeed(msg) => self.broadcast(msg.into()),
            Seek(msg) => self.broadcast(msg.into()),
            UserMessage(msg) => self.broadcast(msg.into()),
            Select(msg) => {
                // a new selection ends whatever file share ran for the old one
                self.fs_end();
                self.broadcast(msg.into())
            }
            FileRequest(msg) => self.fs_file_request(msg),
            FileResponse(msg) => self.fs_file_response(msg),
            VideoShareChange(msg) => self.fs_video_share(msg),
        }
    }

    /// A message of the other role's group reaches a role only when the
    /// core still acts on a role this connection does not have, e.g. while
    /// the core has not yet seen the `ConnectedMsg` of a reconnect. It has no
    /// meaning here.
    pub fn drop_for_other_role(&self, msg: impl std::fmt::Debug) -> Result<()> {
        warn!(?msg, "Dropping a core message meant for the other role");
        Ok(())
    }

    pub fn broadcast(&mut self, msg: NiketsuMessage) -> Result<()> {
        let topic = self.net.topic.clone();
        self.net.swarm.try_broadcast(topic, msg)
    }

    /// Hands a room message from the network to the core.
    pub fn forward(&mut self, msg: impl Into<IncomingMessage>) -> Result<()> {
        self.net.message_sender.send(msg.into())?;
        Ok(())
    }

    pub fn respond_with_err(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = MessageResponse(StatusResponse::Err);
        self.net.swarm.send_message_response(channel, resp)?;
        bail!("Received unexpected direct message: {msg:?}");
    }

    /// Answers a direct request that needs no further handling.
    pub fn acknowledge(&mut self, channel: ResponseChannel<MessageResponse>) -> Result<()> {
        let resp = MessageResponse(StatusResponse::Ok);
        self.net.swarm.send_message_response(channel, resp)
    }

    /// Hands a direct request to the core and acknowledges it.
    pub fn send_to_core(
        &mut self,
        msg: impl Into<IncomingMessage>,
        channel: ResponseChannel<MessageResponse>,
    ) -> Result<()> {
        let resp = match self.net.message_sender.send(msg.into()) {
            Ok(_) => MessageResponse(StatusResponse::Ok),
            Err(_) => MessageResponse(StatusResponse::Err),
        };
        self.net.swarm.send_message_response(channel, resp)
    }
}

/// The session's ends of everything it talks through: the swarm, the
/// channels to the core and the chunk routes. Apart from the rest of the
/// session, so the file share can use it while the session holds the share.
pub(crate) struct Net {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    host: PeerId,
    host_link: HostLink,
    /// Set to end the run loop.
    stop: Option<Stop>,
    core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
    message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
    /// Chunk requests of the local video server.
    chunk_fetches: tokio::sync::mpsc::Receiver<ChunkRequest>,
    /// Where chunk requests of other peers go to the local video provider.
    provide: ChunkRoute,
    chunk_reads: FuturesUnordered<ChunkRead>,
}

impl Net {
    /// Ends the run loop once the current event is handled. The first
    /// reason given is the one reported.
    pub fn stop(&mut self, why: Stop) {
        self.stop.get_or_insert(why);
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

    /// Shows the user a line in the chat that no peer wrote.
    pub fn tell_user(&self, message: &str) {
        let msg = UserMessageMsg {
            actor: arcstr::literal!("server"),
            message: message.to_string(),
        };
        let res = self.message_sender.send(msg.into());
        log_err_msg!(res, "Failed to send message to core");
    }
}
