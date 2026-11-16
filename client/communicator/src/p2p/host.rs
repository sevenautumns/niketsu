use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, mdns};
use niketsu_core::communicator::{
    ConnectedMsg, HostInstruction, IncomingMessage, OutgoingMessage, PeerDisconnectedMsg,
    PeerIntentMsg, RoomStateMsg,
};
use niketsu_core::log_err_msg;
use tracing::{debug, error, info, warn};

use super::links::{Closed, Opened};
use super::member::Membership;
use super::outbox::Outbox;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, FileShareBehaviourEvent, HostLink, MessageRequest,
    MessageResponse, Role, Session, StatusResponse, Stop,
};
use crate::messages::NiketsuMessage;

pub(crate) struct HostRole {
    session: Session,
    relay_peer_id: PeerId,
    /// The listener on the relay's circuit, which lives as long as the
    /// reservation there does. Peers reach the host through it.
    reservation: ListenerId,
    /// Where mDNS found a peer on the local network.
    lan_addrs: HashMap<PeerId, Multiaddr>,
    /// The peers the host took into the room, from their request to join
    /// until their last direct connection closes or a message to them
    /// fails. Only they get what the host sends to the room, through their
    /// outbox.
    members: HashMap<PeerId, Outbox<OutboundRequestId>>,
}

impl HostRole {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        membership: Membership,
        host: PeerId,
        relay_peer_id: PeerId,
        reservation: ListenerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        message_sender
            .send(ConnectedMsg { is_host: true }.into())
            .ok();
        let session = Session::new(
            swarm,
            membership,
            host,
            HostLink::Local,
            core_receiver,
            message_sender,
            chunk_routes,
        );
        Self {
            session,
            relay_peer_id,
            reservation,
            lan_addrs: HashMap::default(),
            members: HashMap::default(),
        }
    }

    /// Dials a peer that waits on its circuit for a direct connection, if
    /// mDNS found it on the local network: no hole to punch there.
    fn dial_on_lan(&mut self, peer_id: PeerId) {
        if !self.session.links.awaits_direct(&peer_id) {
            return;
        }
        let Some(addr) = self.lan_addrs.get(&peer_id) else {
            return;
        };
        match self.session.net.swarm.dial(addr.clone()) {
            Ok(()) => debug!(%peer_id, %addr, "Dialing peer on the local network"),
            Err(err) => warn!(%peer_id, %addr, ?err, "Failed to dial peer on the local network"),
        }
    }

    fn on_mdns(&mut self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(nodes) => {
                debug!(?nodes, "mDNS discovered some nodes");
                // mDNS reports every local address of a peer; one dial is
                // enough.
                let mut preferred: HashMap<PeerId, Multiaddr> = HashMap::new();
                for (peer, addr) in nodes {
                    match preferred.get(&peer) {
                        Some(known) if lan_rank(known) <= lan_rank(&addr) => {}
                        _ => {
                            preferred.insert(peer, addr);
                        }
                    }
                }
                for (peer, addr) in preferred {
                    // A peer that has not dialed the host's circuit yet is
                    // dialed from on_opened.
                    self.lan_addrs.insert(peer, addr);
                    self.dial_on_lan(peer);
                }
            }
            mdns::Event::Expired(nodes) => {
                for (peer, _) in nodes {
                    debug!(%peer, "mDNS node expired");
                    self.lan_addrs.remove(&peer);
                }
            }
        }
    }

    /// Takes a peer that asked to join into the room. A peer joins again
    /// with each session, also while connections of its previous one are
    /// open, and starts over with an empty outbox each time. The core
    /// learns of the member from the status it sends once it is in.
    fn welcome(&mut self, peer_id: PeerId) {
        self.members.insert(peer_id, Outbox::default());
    }

    fn on_core_instruction(&mut self, instruction: HostInstruction) {
        use HostInstruction::*;
        match instruction {
            RoomState(m) => self.publish(m),
            Line(m) => {
                let author = m.author();
                self.send_to_members(m.into(), author)
            }
        }
    }

    /// Sends the room's state to every member.
    fn publish(&mut self, state: RoomStateMsg) {
        let net = &mut self.session.net;
        for (peer, outbox) in &mut self.members {
            outbox.push(state.clone().into(), |msg| net.send_request(peer, msg));
        }
    }

    /// Sends `msg` to every member but its `author`, whose chat shows it
    /// already.
    fn send_to_members(&mut self, msg: NiketsuMessage, author: Option<PeerId>) {
        let net = &mut self.session.net;
        for (peer, outbox) in &mut self.members {
            if Some(*peer) != author {
                outbox.push(msg.clone(), |msg| net.send_request(peer, msg));
            }
        }
    }

    /// A member did not take the message on its way to it: it refused it,
    /// the request timed out or its connection closed. Whether the member
    /// applied it is unknown, and what waits behind it would reach the
    /// member out of step, so the host drops the member and closes its
    /// connections. The member's session ends with them; it connects anew
    /// and is greeted with the room.
    fn drop_member(&mut self, peer: PeerId, why: &dyn std::fmt::Debug) {
        warn!(%peer, ?why, "A member did not take a message; dropping it from the room");
        self.members.remove(&peer);
        if self.session.net.swarm.disconnect_peer_id(peer).is_err() {
            debug!(%peer, "The dropped member had no connection left");
        }
    }
}

/// Which of a peer's local addresses to dial first: TCP, then QUIC.
fn lan_rank(addr: &Multiaddr) -> u8 {
    if addr.iter().any(|p| matches!(p, Protocol::Tcp(_))) {
        0
    } else if addr.iter().any(|p| matches!(p, Protocol::QuicV1)) {
        1
    } else {
        2
    }
}

#[async_trait]
impl Role for HostRole {
    fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    fn on_opened(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        endpoint: ConnectedPoint,
        opened: Opened,
    ) {
        if peer_id == self.relay_peer_id {
            return;
        }
        // A circuit is the client's to close: the client gives up on a host
        // whose circuit closes before the direct connection shows up there.
        // A connection makes no member; a peer joins with a request.
        match opened {
            Opened::Rendezvous => {
                debug!(%peer_id, "Client connected via relay; awaiting direct connection");
                self.dial_on_lan(peer_id);
            }
            Opened::SpareCircuit => debug!(%peer_id, "Relay circuit beside a direct connection"),
            Opened::Linked { .. } | Opened::AnotherDirect => {
                info!(%peer_id, ?endpoint, "Direct connection established");
            }
        }
    }

    fn on_closed(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        cause: Option<ConnectionError>,
        closed: Closed,
    ) {
        if peer_id == self.relay_peer_id {
            // The swarm is asked, as the connection to the relay is older
            // than the session and its links. Every connection to the relay
            // is a direct one.
            if !self.session.net.swarm.is_connected(&peer_id) {
                error!(?cause, "Connection of host to relay server closed");
                self.session.net.stop(Stop::RelayLost);
            }
            return;
        }
        match closed {
            // the core forgets a peer that joined, and ignores one that never
            // did
            Closed::Unlinked => {
                debug!(%peer_id, ?cause, "Peer disconnected");
                self.members.remove(&peer_id);
                let res = self.session.forward(PeerDisconnectedMsg { peer: peer_id });
                log_err_msg!(res, "Failed to report disconnected peer to core");
                // a circuit it left open is a peer starting over
                self.dial_on_lan(peer_id);
            }
            Closed::RendezvousLost => debug!(%peer_id, "Client left before a direct connection"),
            Closed::Unchanged => {}
        }
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        use BehaviourEvent::*;
        use FileShareBehaviourEvent as F;
        match event {
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            // Without the reservation nobody can join the room, also when
            // the connection to the relay is still up.
            SwarmEvent::ListenerClosed {
                listener_id,
                reason,
                ..
            } if listener_id == self.reservation => {
                error!(?reason, "Reservation of host on relay server closed");
                self.session.net.stop(Stop::RelayLost);
            }
            other => debug!(event = ?other, "Received not captured event"),
        }
    }

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!(message = ?msg, peer = ?peer_id, "Handling request message from swarm");
        use NiketsuMessage::*;
        match msg {
            Join => {
                self.welcome(peer_id);
                self.session.acknowledge(channel)
            }
            Intent { intent } => {
                let msg = PeerIntentMsg {
                    peer: peer_id,
                    intent,
                };
                self.session.send_to_core(msg, channel)
            }
            other => self.session.respond_with_err(other, channel),
        }
    }

    /// A member answered the message on its way to it, or a request of a
    /// session it has left.
    fn on_msg_response(
        &mut self,
        peer: PeerId,
        request_id: OutboundRequestId,
        response: MessageResponse,
    ) {
        let net = &mut self.session.net;
        let Some(outbox) = self.members.get_mut(&peer) else {
            return debug!(%peer, ?response, "Answer from a peer outside the room");
        };
        match response.0 {
            StatusResponse::Ok => {
                if !outbox.answered(&request_id, |msg| net.send_request(&peer, msg)) {
                    debug!(%peer, "Answer to a request of an earlier session");
                }
            }
            StatusResponse::Err if outbox.is_on_its_way(&request_id) => {
                self.drop_member(peer, &response)
            }
            StatusResponse::Err => debug!(%peer, "Refusal of a request of an earlier session"),
        }
    }

    fn on_msg_req_resp_other(
        &mut self,
        event: request_response::Event<MessageRequest, MessageResponse>,
    ) {
        match event {
            request_response::Event::OutboundFailure {
                peer,
                request_id,
                error,
                ..
            } if self
                .members
                .get(&peer)
                .is_some_and(|outbox| outbox.is_on_its_way(&request_id)) =>
            {
                self.drop_member(peer, &error)
            }
            other => debug!(
                ?other,
                "Received request response event that is not handled"
            ),
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Result<()> {
        debug!(host = %self.session.net.host, ?msg, "Handling core message");
        match msg {
            OutgoingMessage::Host(m) => {
                self.on_core_instruction(m);
                Ok(())
            }
            OutgoingMessage::Client(m) => self.session.drop_for_other_role(m),
        }
    }
}

/// The host's fan-out with real swarms over the memory transport: a host
/// role and test peers next to it, which speak the message protocol
/// directly.
#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use futures::StreamExt;
    use libp2p::core::muxing::StreamMuxerBox;
    use libp2p::core::transport::{Boxed, MemoryTransport};
    use libp2p::core::upgrade::Version;
    use libp2p::request_response::{Event, Message, ProtocolSupport};
    use libp2p::{StreamProtocol, Transport, noise, yamux};
    use niketsu_core::chunk::{ChunkRequest, ChunkRoute, ProvideRoute, SharedFile};
    use niketsu_core::communicator::{Intent, LineKind, LineMsg};
    use niketsu_core::playlist::Playlist;
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::*;
    use crate::p2p::file_share::{ChunkBytes, FileShareRequest, FileShareResponseResult};
    use crate::p2p::member::Tagged;
    use crate::p2p::messaging;
    use crate::timing::MESSAGE_REQUEST_TIMEOUT;

    const ROOM: &str = "room";
    const PASSWORD: &str = "secret";

    /// Well below the time a request may wait for its answer, so a member
    /// that waits on another's answer misses it.
    const PROMPT: Duration = Duration::from_secs(2);

    async fn eventually(what: &str, holds: impl Fn() -> bool) {
        eventually_within(PROMPT, what, holds).await
    }

    async fn eventually_within(patience: Duration, what: &str, holds: impl Fn() -> bool) {
        let deadline = Instant::now() + patience;
        while !holds() {
            assert!(Instant::now() < deadline, "never happened: {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn memory(
        key: &libp2p::identity::Keypair,
    ) -> std::result::Result<
        Boxed<(PeerId, StreamMuxerBox)>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Ok(MemoryTransport::default()
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(key)?)
            .multiplex(yamux::Config::default())
            .boxed())
    }

    /// A host role listening on the memory transport, and its ends of the
    /// channels to the core.
    async fn host() -> (
        PeerId,
        Multiaddr,
        UnboundedSender<OutgoingMessage>,
        Arc<Mutex<Vec<IncomingMessage>>>,
    ) {
        host_providing(ProvideRoute::default()).await
    }

    /// Like [`host`], for a host that offers what `provide` serves.
    async fn host_providing(
        provide: ProvideRoute,
    ) -> (
        PeerId,
        Multiaddr,
        UnboundedSender<OutgoingMessage>,
        Arc<Mutex<Vec<IncomingMessage>>>,
    ) {
        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_other_transport(memory)
            .unwrap()
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .unwrap()
            .with_behaviour(|key, relay_client| Ok(Behaviour::new(key, relay_client, false)?))
            .unwrap()
            .build();
        let host = *swarm.local_peer_id();
        let listener = swarm.listen_on("/memory/0".parse().unwrap()).unwrap();
        let addr = loop {
            if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                break address.with(Protocol::P2p(host));
            }
        };
        let (core, core_receiver) = unbounded_channel();
        let (message_sender, mut to_core) = unbounded_channel();
        let routes = ChunkRoutes {
            fetch: ChunkRoute::default(),
            provide,
        };
        let membership = Membership::new(ROOM.into(), PASSWORD.into(), &host);
        let relay = PeerId::random();
        let mut role = HostRole::new(
            swarm,
            membership,
            host,
            relay,
            listener,
            core_receiver,
            message_sender,
            routes,
        );
        tokio::spawn(async move { role.run().await });
        let seen = Arc::<Mutex<Vec<IncomingMessage>>>::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Some(msg) = to_core.recv().await {
                log.lock().unwrap().push(msg);
            }
        });
        (host, addr, core, seen)
    }

    /// A peer connected to the host, and what the host sent it.
    struct Neighbour {
        peer: PeerId,
        received: Arc<Mutex<Vec<NiketsuMessage>>>,
        /// Whether the host took the peer into the room: it answered the
        /// request to join.
        joined: Arc<AtomicBool>,
        outbox: UnboundedSender<NiketsuMessage>,
    }

    impl Neighbour {
        /// Connects to the host knowing the room by `password`, asks to
        /// join if `joins`, and answers what the host sends if `answers`.
        fn start(
            host: PeerId,
            addr: &Multiaddr,
            password: &str,
            joins: bool,
            answers: bool,
        ) -> Self {
            let mut swarm = libp2p::SwarmBuilder::with_new_identity()
                .with_tokio()
                .with_other_transport(memory)
                .unwrap()
                .with_behaviour(|_| messaging())
                .unwrap()
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();
            let peer = *swarm.local_peer_id();
            let membership = Membership::new(ROOM.into(), password.into(), &peer);
            swarm.dial(addr.clone()).unwrap();
            let received = Arc::<Mutex<Vec<NiketsuMessage>>>::default();
            let (outbox, mut requests) = unbounded_channel();
            if joins {
                outbox.send(NiketsuMessage::Join).unwrap();
            }
            let log = Arc::clone(&received);
            let joined = Arc::<AtomicBool>::default();
            let took_in = Arc::clone(&joined);
            tokio::spawn(async move {
                let mut unanswered = Vec::new();
                let mut join = None;
                loop {
                    tokio::select! {
                        Some(msg) = requests.recv() => {
                            let joining = msg == NiketsuMessage::Join;
                            let id = swarm.behaviour_mut().send_request(&host, membership.tag(msg));
                            if joining {
                                join = Some(id);
                            }
                        }
                        event = swarm.select_next_some() => {
                            let SwarmEvent::Behaviour(Event::Message { message, .. }) = event else {
                                continue;
                            };
                            let (request, channel) = match message {
                                Message::Request { request, channel, .. } => (request, channel),
                                Message::Response { request_id, response } => {
                                    if Some(request_id) == join && response.0 == StatusResponse::Ok {
                                        took_in.store(true, Ordering::SeqCst);
                                    }
                                    continue;
                                }
                            };
                            log.lock().unwrap().push(request.msg);
                            if answers {
                                let ok = MessageResponse(StatusResponse::Ok);
                                swarm.behaviour_mut().send_response(channel, ok).unwrap();
                            } else {
                                unanswered.push(channel);
                            }
                        }
                    }
                }
            });
            Self {
                peer,
                received,
                joined,
                outbox,
            }
        }

        fn joined(&self) -> bool {
            self.joined.load(Ordering::SeqCst)
        }

        fn received(&self) -> Vec<NiketsuMessage> {
            self.received.lock().unwrap().clone()
        }

        /// The playlists of the room states this peer got.
        fn playlists(&self) -> Vec<Option<Playlist>> {
            let playlist = |msg: NiketsuMessage| match msg {
                NiketsuMessage::RoomState(state) => Some(state.playlist),
                _ => None,
            };
            self.received().into_iter().filter_map(playlist).collect()
        }

        fn has(&self, msg: &NiketsuMessage) -> bool {
            self.received().contains(msg)
        }

        /// The peer asks the host for `intent`.
        fn ask(&self, intent: Intent) {
            self.outbox.send(intent.into()).unwrap();
        }
    }

    /// A chat line of the user at `author`.
    fn chat(line: &str, author: PeerId) -> LineMsg {
        LineMsg {
            actor: "someone".into(),
            message: line.into(),
            kind: LineKind::Chat { author },
        }
    }

    /// The `n`th state the host publishes.
    fn state(n: u64) -> RoomStateMsg {
        RoomStateMsg {
            jumps: n,
            ..Default::default()
        }
    }

    fn action(line: &str) -> LineMsg {
        LineMsg {
            actor: "someone".into(),
            message: line.into(),
            kind: LineKind::Action,
        }
    }

    /// The `n`th state, with the playlist `name`.
    fn with_playlist(n: u64, name: &str) -> RoomStateMsg {
        RoomStateMsg {
            playlist: Some(Playlist::from_iter([name])),
            ..state(n)
        }
    }

    fn departed(to_core: &Mutex<Vec<IncomingMessage>>) -> HashSet<PeerId> {
        let peer = |msg: &IncomingMessage| match msg {
            IncomingMessage::PeerDisconnected(m) => Some(m.peer),
            _ => None,
        };
        to_core.lock().unwrap().iter().filter_map(peer).collect()
    }

    /// Whether the host handed its core `intent` of the user at `peer`.
    fn asked(to_core: &Mutex<Vec<IncomingMessage>>, peer: PeerId, intent: &Intent) -> bool {
        let asked = |msg: &IncomingMessage| matches!(msg, IncomingMessage::PeerIntent(m) if m.peer == peer && m.intent == *intent);
        to_core.lock().unwrap().iter().any(asked)
    }

    /// What the core hands the host role for it to send `msg` to the room.
    fn from_core(msg: &NiketsuMessage) -> OutgoingMessage {
        match msg.clone() {
            NiketsuMessage::RoomState(m) => m.into(),
            NiketsuMessage::Line(m) => m.into(),
            other => unreachable!("not for the room: {other:?}"),
        }
    }

    /// Whether `received` is `sent` with some messages left out, the rest
    /// in the order sent.
    fn in_order(received: &[NiketsuMessage], sent: &[NiketsuMessage]) -> bool {
        let mut sent = sent.iter();
        received.iter().all(|msg| sent.any(|s| s == msg))
    }

    /// Every member gets what the host sends the room in the order sent:
    /// every line, and of the room states at least the newest, a waiting
    /// one giving way to a newer one. A chat line goes to everyone but its
    /// author. A member's intent reaches the host's core. A peer next to
    /// the host that is not a member gets nothing, whether it knows the
    /// room and never joined or was refused, and what a refused peer asks
    /// never reaches the core.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_host_sends_each_member_the_room_in_order_and_outsiders_nothing() {
        let (host, addr, core, to_core) = host().await;
        let one = Neighbour::start(host, &addr, PASSWORD, true, true);
        let two = Neighbour::start(host, &addr, PASSWORD, true, true);
        let lurker = Neighbour::start(host, &addr, PASSWORD, false, true);
        let intruder = Neighbour::start(host, &addr, "guess", true, true);
        eventually("the host takes in both members", || {
            one.joined() && two.joined()
        })
        .await;

        let from_one = Intent::Chat("from one".into());
        let from_intruder = Intent::Chat("from the intruder".into());
        one.ask(from_one.clone());
        intruder.ask(from_intruder.clone());
        eventually("the host's core hears the member", || {
            asked(&to_core, one.peer, &from_one)
        })
        .await;

        let mut sent: Vec<NiketsuMessage> = Vec::new();
        for i in 1..=30 {
            sent.push(state(i).into());
            if i % 3 == 0 {
                sent.push(action(&format!("action {i}")).into());
            }
            if i % 5 == 0 {
                sent.push(chat(&format!("chat {i}"), host).into());
            }
            if i == 14 {
                sent.push(chat("from one", one.peer).into());
            }
        }
        for msg in &sent {
            core.send(from_core(msg)).unwrap();
        }
        let last = NiketsuMessage::RoomState(state(30));
        let final_line = sent.last().unwrap();
        let ones_line = NiketsuMessage::Line(chat("from one", one.peer));
        for member in [&one, &two] {
            eventually("every member gets the last message", || {
                member.has(final_line)
            })
            .await;
            let received = member.received();
            assert!(in_order(&received, &sent), "out of order: {received:#?}");
            let line_for_member = |msg: &&NiketsuMessage| match msg {
                NiketsuMessage::Line(line) => line.author() != Some(member.peer),
                _ => false,
            };
            assert!(
                sent.iter()
                    .filter(line_for_member)
                    .all(|line| received.contains(line)),
                "a line is missing: {received:#?}"
            );
            let newest = received
                .iter()
                .rfind(|msg| matches!(msg, NiketsuMessage::RoomState(_)));
            assert_eq!(newest, Some(&last), "the newest state is the last");
        }

        assert!(two.has(&ones_line));
        assert!(!one.has(&ones_line), "the author shows its line already");
        for outsider in [&lurker, &intruder] {
            assert_eq!(outsider.received(), [], "an outsider got the room");
            assert!(!outsider.joined());
        }
        assert!(!asked(&to_core, intruder.peer, &from_intruder));
    }

    /// A member that does not answer gets nothing past the message on its
    /// way and holds up no one. Once that request times out, the host drops
    /// the member and closes its connection, which the core hears as the
    /// member leaving.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_that_does_not_answer_holds_up_no_one_and_is_dropped() {
        let (host, addr, core, to_core) = host().await;
        let quick = Neighbour::start(host, &addr, PASSWORD, true, true);
        let silent = Neighbour::start(host, &addr, PASSWORD, true, false);
        eventually("the host takes in both members", || {
            quick.joined() && silent.joined()
        })
        .await;

        let room = [
            NiketsuMessage::RoomState(state(1)),
            NiketsuMessage::Line(action("someone paused playback")),
            NiketsuMessage::RoomState(state(2)),
            NiketsuMessage::Line(chat("from the host", host)),
        ];
        for msg in &room {
            core.send(from_core(msg)).unwrap();
        }
        eventually("the quick member gets everything", || {
            quick.received() == room
        })
        .await;
        // whatever the host sends the silent member is on its way by now
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(silent.received(), room[..1], "one message on its way");

        eventually_within(
            MESSAGE_REQUEST_TIMEOUT + PROMPT,
            "the host drops the silent member",
            || departed(&to_core).contains(&silent.peer),
        )
        .await;
        assert!(!departed(&to_core).contains(&quick.peer));
        core.send(state(3).into()).unwrap();
        let after = NiketsuMessage::RoomState(state(3));
        eventually("the quick member goes on", || quick.has(&after)).await;
        assert_eq!(silent.received(), room[..1]);
    }

    /// What a peer that streams speaks: the file share protocol.
    type FileShareSide =
        request_response::cbor::Behaviour<Tagged<FileShareRequest>, FileShareResponseResult>;

    /// A peer next to `host` that knows the room's password, speaking the
    /// file share protocol only.
    fn requester(addr: &Multiaddr) -> (Swarm<FileShareSide>, Membership) {
        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_other_transport(memory)
            .unwrap()
            .with_behaviour(|_| {
                FileShareSide::new(
                    [(StreamProtocol::new("/fileshare/1"), ProtocolSupport::Full)],
                    request_response::Config::default(),
                )
            })
            .unwrap()
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();
        let membership = Membership::new(ROOM.into(), PASSWORD.into(), swarm.local_peer_id());
        swarm.dial(addr.clone()).unwrap();
        (swarm, membership)
    }

    /// What `host` answers when asked for the first ten bytes of `video`.
    async fn ask_for_chunk(
        swarm: &mut Swarm<FileShareSide>,
        membership: &Membership,
        host: PeerId,
        video: &str,
    ) -> FileShareResponseResult {
        let msg = FileShareRequest {
            video: video.into(),
            start: 0,
            len: 10,
        };
        let id = swarm
            .behaviour_mut()
            .send_request(&host, membership.tag(msg));
        let answer = async {
            loop {
                match swarm.select_next_some().await {
                    SwarmEvent::Behaviour(Event::Message {
                        message:
                            Message::Response {
                                request_id,
                                response,
                            },
                        ..
                    }) if request_id == id => return response,
                    SwarmEvent::Behaviour(Event::OutboundFailure {
                        request_id, error, ..
                    }) if request_id == id => panic!("the chunk request failed: {error:?}"),
                    _ => {}
                }
            }
        };
        tokio::time::timeout(PROMPT, answer)
            .await
            .expect("the host never answered the chunk request")
    }

    /// The communicator answers a chunk request from its provide route by
    /// itself: the route's provider reads a chunk of the file the route
    /// serves. A request for a file the route does not serve is turned
    /// down, and so is every request once the route closed.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_chunk_request_is_answered_from_the_provide_route() {
        let provide = ProvideRoute::default();
        let (sender, mut requests) = tokio::sync::mpsc::channel::<ChunkRequest>(1);
        tokio::spawn(async move {
            while let Some(request) = requests.recv().await {
                request.reply.send((0..10).collect()).ok();
            }
        });
        let file = SharedFile {
            name: "ep1".into(),
            size: 1_300_000,
        };
        provide.open(file, sender);
        let (host, addr, _core, _to_core) = host_providing(provide.clone()).await;
        let (mut swarm, membership) = requester(&addr);

        let answer = ask_for_chunk(&mut swarm, &membership, host, "ep1").await;
        let Ok(ChunkBytes(bytes)) = answer else {
            panic!("the offered file was not served: {answer:?}");
        };
        assert_eq!(bytes, (0..10).collect::<Vec<u8>>());

        let answer = ask_for_chunk(&mut swarm, &membership, host, "ep2").await;
        assert!(answer.is_err(), "a file not offered was served");

        provide.close();
        let answer = ask_for_chunk(&mut swarm, &membership, host, "ep1").await;
        assert!(answer.is_err(), "a withdrawn offer was served");
    }

    /// A room state carries the playlist to a member until the member
    /// answered one that carried it, and a new playlist once again. A
    /// member that joins later gets the playlist with its first state.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_gets_the_playlist_until_it_took_it() {
        let (host, addr, core, _to_core) = host().await;
        let early = Neighbour::start(host, &addr, PASSWORD, true, true);
        eventually("the host takes in the member", || early.joined()).await;

        let states = [
            with_playlist(1, "ep1"),
            with_playlist(2, "ep1"),
            with_playlist(3, "ep3"),
            with_playlist(4, "ep3"),
        ];
        for (i, state) in states.into_iter().enumerate() {
            core.send(state.into()).unwrap();
            eventually("the member gets the state", || {
                early.playlists().len() == i + 1
            })
            .await;
        }
        let ep1 = Some(Playlist::from_iter(["ep1"]));
        let ep3 = Some(Playlist::from_iter(["ep3"]));
        assert_eq!(early.playlists(), [ep1, None, ep3.clone(), None]);

        let late = Neighbour::start(host, &addr, PASSWORD, true, true);
        eventually("the host takes in the late member", || late.joined()).await;
        core.send(with_playlist(5, "ep3").into()).unwrap();
        eventually("the late member gets the playlist", || {
            late.playlists() == [ep3.clone()]
        })
        .await;
        eventually("the early member gets the state without", || {
            early.playlists().len() == 5
        })
        .await;
        assert_eq!(early.playlists()[4], None);
    }
}
