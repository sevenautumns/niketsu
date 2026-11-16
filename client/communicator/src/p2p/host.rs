use std::collections::HashMap;

use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{ConnectionError, ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId, Swarm, mdns, ping};
use niketsu_core::communicator::{
    ConnectedMsg, IncomingMessage, OutgoingMessage, PeerDisconnectedMsg, PeerIntentMsg,
};
use tracing::{debug, error, info, warn};

use super::links::{Closed, Opened};
use super::member::Membership;
use super::stream::StreamEvent;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, FileShareBehaviourEvent, HostLink, Role, Session, Stop,
    TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

/// The host of a room. Its members are the peers whose stream is open
/// (see [`super::stream`]): each gets what the host sends the room, from
/// the moment the host admitted its stream until that stream ends.
pub(crate) struct HostRole {
    session: Session,
    relay_peer_id: PeerId,
    /// The listener on the relay's circuit, which lives as long as the
    /// reservation there does. Peers reach the host through it.
    reservation: ListenerId,
    /// Where mDNS found a peer on the local network.
    lan_addrs: HashMap<PeerId, Multiaddr>,
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
        let mut session = Session::new(
            swarm,
            membership,
            host,
            HostLink::Local,
            core_receiver,
            message_sender,
            chunk_routes,
        );
        session.streams.accept();
        Self {
            session,
            relay_peer_id,
            reservation,
            lan_addrs: HashMap::default(),
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

    /// A peer that does not answer its pings is gone, and a connection
    /// that stays open to it would keep its stream open: nothing fails on
    /// a stream to a peer that went silent until much was written to it.
    fn on_ping(&mut self, event: ping::Event) {
        if event.peer == self.relay_peer_id {
            return;
        }
        match event.result {
            // a peer that speaks no ping has not gone silent
            Ok(_) | Err(ping::Failure::Unsupported) => {}
            Err(error) => {
                warn!(peer = %event.peer, %error, "Ping to peer failed, closing connection");
                self.session.net.swarm.close_connection(event.connection);
            }
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
        // A connection makes no member; a peer joins with its stream.
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
            // a member's stream ends with the connection it is on
            Closed::Unlinked => {
                debug!(%peer_id, ?cause, "Peer disconnected");
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
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(FileShare(F::Mdns(e))) => self.on_mdns(e),
            SwarmEvent::Behaviour(Transport(T::Ping(e))) => self.on_ping(e),
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

    /// The core learns of a member from the status the member sends once
    /// its stream is open, and of its leaving when that stream ends,
    /// whatever the cause: the member ended it, its connection closed, or
    /// a write to it did not get through. Whether the member took what was
    /// written last is unknown then; it connects anew and is greeted with
    /// the room.
    fn on_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Opened(peer) => info!(%peer, "A peer joined the room"),
            StreamEvent::Refused(peer) => warn!(%peer, "Turned down a peer without the room's tag"),
            StreamEvent::Received(peer, NiketsuMessage::Intent { intent }) => {
                self.session.forward(PeerIntentMsg { peer, intent })
            }
            StreamEvent::Received(peer, other) => {
                debug!(%peer, ?other, "Not for a member to send")
            }
            StreamEvent::Ended(peer) => {
                info!(%peer, "A member's stream ended");
                self.session.forward(PeerDisconnectedMsg { peer });
            }
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Option<OutgoingMessage> {
        let streams = &mut self.session.streams;
        match msg {
            OutgoingMessage::RoomState(state) => {
                streams.send(NiketsuMessage::RoomState(state), |_| true)
            }
            // the author's chat shows its line already
            OutgoingMessage::Line(line) => {
                let author = line.author();
                streams.send(NiketsuMessage::Line(line), |peer| Some(*peer) != author)
            }
            other => return Some(other),
        }
        None
    }
}

/// The host's fan-out with real swarms over the memory transport: a host
/// role and test peers next to it, which speak the room's stream protocol
/// directly.
#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use futures::{AsyncReadExt, StreamExt};
    use libp2p::core::muxing::StreamMuxerBox;
    use libp2p::core::transport::{Boxed, MemoryTransport};
    use libp2p::core::upgrade::Version;
    use libp2p::request_response::{self, Event, Message, ProtocolSupport};
    use libp2p::{StreamProtocol, Transport, noise, yamux};
    use libp2p_stream::Control;
    use niketsu_core::chunk::{ChunkRequest, ChunkRoute, ProvideRoute, SharedFile};
    use niketsu_core::communicator::{Intent, LineKind, LineMsg, RoomStateMsg};
    use niketsu_core::playlist::Playlist;
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::*;
    use crate::p2p::file_share::{ChunkBytes, FileShareRequest, FileShareResponseResult};
    use crate::p2p::member::Tagged;
    use crate::p2p::stream::{PROTOCOL, read_frame, write_frame};
    use crate::timing::MESSAGE_REQUEST_TIMEOUT;

    const ROOM: &str = "room";
    const PASSWORD: &str = "secret";

    /// Well below the time a write may take, so a member that waits on
    /// another's stream misses it.
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

    /// A peer with a direct connection to the host.
    struct Neighbour {
        host: PeerId,
        peer: PeerId,
        control: Control,
    }

    impl Neighbour {
        async fn connect(host: PeerId, addr: &Multiaddr) -> Self {
            let mut swarm = libp2p::SwarmBuilder::with_new_identity()
                .with_tokio()
                .with_other_transport(memory)
                .unwrap()
                .with_behaviour(|_| libp2p_stream::Behaviour::new())
                .unwrap()
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();
            let peer = *swarm.local_peer_id();
            let control = swarm.behaviour().new_control();
            swarm.dial(addr.clone()).unwrap();
            loop {
                if let SwarmEvent::ConnectionEstablished { .. } = swarm.select_next_some().await {
                    break;
                }
            }
            tokio::spawn(async move {
                loop {
                    swarm.select_next_some().await;
                }
            });
            Self {
                host,
                peer,
                control,
            }
        }

        /// Opens a stream to the host and presents the tag of this peer in
        /// the room with `password`. With `reads`, what the host writes is
        /// taken off the stream.
        async fn join(&mut self, password: &str, reads: bool) -> Seat {
            let membership = Membership::new(ROOM.into(), password.into(), &self.peer);
            let stream = self.control.open_stream(self.host, PROTOCOL).await.unwrap();
            let (mut read, mut write) = stream.split();
            write_frame(&mut write, &membership.own_tag())
                .await
                .unwrap();
            let received = Arc::<Mutex<Vec<NiketsuMessage>>>::default();
            let log = Arc::clone(&received);
            let (outbox, mut asked) = unbounded_channel::<NiketsuMessage>();
            let writing = tokio::spawn(async move {
                while let Some(msg) = asked.recv().await {
                    if write_frame(&mut write, &msg).await.is_err() {
                        return;
                    }
                }
                // holds the stream open
                std::future::pending().await
            });
            let reading = tokio::spawn(async move {
                if !reads {
                    // holds the stream open without taking anything
                    return std::future::pending().await;
                }
                while let Ok(msg) = read_frame(&mut read).await {
                    log.lock().unwrap().push(msg);
                }
            });
            Seat {
                received,
                outbox,
                stream: [writing.abort_handle(), reading.abort_handle()],
            }
        }

        /// Joins the room and reads what the host writes.
        async fn member(host: PeerId, addr: &Multiaddr) -> (Self, Seat) {
            let mut neighbour = Self::connect(host, addr).await;
            let seat = neighbour.join(PASSWORD, true).await;
            (neighbour, seat)
        }
    }

    /// A peer's end of one stream to the host. The stream ends when this
    /// is dropped.
    struct Seat {
        received: Arc<Mutex<Vec<NiketsuMessage>>>,
        outbox: UnboundedSender<NiketsuMessage>,
        /// The tasks that hold the two halves of the stream.
        stream: [tokio::task::AbortHandle; 2],
    }

    impl Drop for Seat {
        fn drop(&mut self) {
            for half in &self.stream {
                half.abort();
            }
        }
    }

    impl Seat {
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

    /// The peer asks for `intent` and the host's core hears it: the host
    /// has admitted the peer's stream by then.
    async fn heard(
        to_core: &Mutex<Vec<IncomingMessage>>,
        neighbour: &Neighbour,
        seat: &Seat,
        intent: Intent,
    ) {
        seat.ask(intent.clone());
        eventually("the host's core hears the member", || {
            asked(to_core, neighbour.peer, &intent)
        })
        .await;
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
    /// author. A member's intents reach the host's core in the order sent.
    /// A peer next to the host that is not a member gets nothing, whether
    /// it never opened a stream or presented a wrong tag, and what a
    /// refused peer asks never reaches the core.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_host_sends_each_member_the_room_in_order_and_outsiders_nothing() {
        let (host, addr, core, to_core) = host().await;
        let (one, ones) = Neighbour::member(host, &addr).await;
        let (two, twos) = Neighbour::member(host, &addr).await;
        let _lurker = Neighbour::connect(host, &addr).await;
        let mut intruder = Neighbour::connect(host, &addr).await;
        let intruders = intruder.join("guess", true).await;

        let from_intruder = Intent::Chat("from the intruder".into());
        intruders.ask(from_intruder.clone());
        let from_one: Vec<Intent> = (0..20).map(|i| Intent::Chat(format!("{i}"))).collect();
        for intent in &from_one {
            ones.ask(intent.clone());
        }
        heard(&to_core, &one, &ones, Intent::Chat("from one".into())).await;
        heard(&to_core, &two, &twos, Intent::Chat("from two".into())).await;
        let from = |peer: PeerId| {
            let intent = |msg: &IncomingMessage| match msg {
                IncomingMessage::PeerIntent(m) if m.peer == peer => Some(m.intent.clone()),
                _ => None,
            };
            let heard = to_core.lock().unwrap();
            heard.iter().filter_map(intent).collect::<Vec<_>>()
        };
        assert_eq!(from(one.peer)[..20], from_one, "in the order sent");

        let mut sent: Vec<NiketsuMessage> = Vec::new();
        for i in 1..=30 {
            sent.push(NiketsuMessage::RoomState(state(i)));
            if i % 3 == 0 {
                sent.push(NiketsuMessage::Line(action(&format!("action {i}"))));
            }
            if i % 5 == 0 {
                sent.push(NiketsuMessage::Line(chat(&format!("chat {i}"), host)));
            }
            if i == 14 {
                sent.push(NiketsuMessage::Line(chat("from one", one.peer)));
            }
        }
        for msg in &sent {
            core.send(from_core(msg)).unwrap();
        }
        let last = NiketsuMessage::RoomState(state(30));
        let final_line = sent.last().unwrap();
        let ones_line = NiketsuMessage::Line(chat("from one", one.peer));
        for (member, seat) in [(&one, &ones), (&two, &twos)] {
            eventually("every member gets the last message", || {
                seat.has(final_line)
            })
            .await;
            let received = seat.received();
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

        assert!(twos.has(&ones_line));
        assert!(!ones.has(&ones_line), "the author shows its line already");
        assert_eq!(intruders.received(), [], "an outsider got the room");
        assert_eq!(from(intruder.peer), []);
    }

    /// A member that takes nothing off its stream holds up no one. Once a
    /// write to it does not finish in time, the host ends its stream,
    /// which the core hears as the member leaving.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_that_takes_nothing_holds_up_no_one_and_is_dropped() {
        let (host, addr, core, to_core) = host().await;
        let (quick, quicks) = Neighbour::member(host, &addr).await;
        let mut stuck = Neighbour::connect(host, &addr).await;
        let stucks = stuck.join(PASSWORD, false).await;
        heard(&to_core, &quick, &quicks, Intent::Pause).await;
        heard(&to_core, &stuck, &stucks, Intent::Pause).await;

        // more than a stream takes while nobody reads from it
        let room: Vec<NiketsuMessage> = (0..8)
            .map(|i| NiketsuMessage::Line(chat(&format!("{i}").repeat(200_000), host)))
            .collect();
        for msg in &room {
            core.send(from_core(msg)).unwrap();
        }
        eventually("the quick member gets everything", || {
            quicks.received() == room
        })
        .await;
        assert!(!departed(&to_core).contains(&stuck.peer));

        eventually_within(
            MESSAGE_REQUEST_TIMEOUT + PROMPT,
            "the host drops the member that takes nothing",
            || departed(&to_core).contains(&stuck.peer),
        )
        .await;
        assert!(!departed(&to_core).contains(&quick.peer));
        core.send(state(3).into()).unwrap();
        let after = NiketsuMessage::RoomState(state(3));
        eventually("the quick member goes on", || quicks.has(&after)).await;
    }

    /// The core hears a member leave when its stream ends. A member that
    /// joins again while its earlier stream is open is the same member to
    /// the core: its new stream gets the room, and the end of the earlier
    /// one is no leaving.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_leaves_with_its_stream_and_not_with_one_it_replaced() {
        let (host, addr, core, to_core) = host().await;
        let (mut member, earlier) = Neighbour::member(host, &addr).await;
        heard(&to_core, &member, &earlier, Intent::Pause).await;

        let again = member.join(PASSWORD, true).await;
        heard(&to_core, &member, &again, Intent::Start).await;
        drop(earlier);
        core.send(state(1).into()).unwrap();
        let first = NiketsuMessage::RoomState(state(1));
        eventually("the new stream gets the room", || again.has(&first)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(departed(&to_core), HashSet::new());

        drop(again);
        eventually("the core hears the member leave", || {
            departed(&to_core).contains(&member.peer)
        })
        .await;
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

    /// A room state carries the playlist to a member once, and a new
    /// playlist once again. A member that joins later gets the playlist
    /// with its first state.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_member_gets_each_playlist_once() {
        let (host, addr, core, to_core) = host().await;
        let (early, earlys) = Neighbour::member(host, &addr).await;
        heard(&to_core, &early, &earlys, Intent::Pause).await;

        let states = [
            with_playlist(1, "ep1"),
            with_playlist(2, "ep1"),
            with_playlist(3, "ep3"),
            with_playlist(4, "ep3"),
        ];
        for (i, state) in states.into_iter().enumerate() {
            core.send(state.into()).unwrap();
            eventually("the member gets the state", || {
                earlys.playlists().len() == i + 1
            })
            .await;
        }
        let ep1 = Some(Playlist::from_iter(["ep1"]));
        let ep3 = Some(Playlist::from_iter(["ep3"]));
        assert_eq!(earlys.playlists(), [ep1, None, ep3.clone(), None]);

        let (late, lates) = Neighbour::member(host, &addr).await;
        heard(&to_core, &late, &lates, Intent::Pause).await;
        core.send(with_playlist(5, "ep3").into()).unwrap();
        eventually("the late member gets the playlist", || {
            lates.playlists() == [ep3.clone()]
        })
        .await;
        eventually("the early member gets the state without", || {
            earlys.playlists().len() == 5
        })
        .await;
        assert_eq!(earlys.playlists()[4], None);
    }
}
