use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, ping};
use niketsu_core::communicator::{
    ConnectedMsg, HostRoundTripMsg, IncomingMessage, OutgoingMessage,
};
use tracing::{debug, info, warn};

use super::links::{Closed, Opened};
use super::member::Membership;
use super::stream::StreamEvent;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, HostLink, Role, Session, Stop, TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct ClientRole {
    session: Session,
    /// The listener on the host's circuit, which lives as long as the
    /// reservation on the host's relay does.
    host_reservation: Option<ListenerId>,
}

impl ClientRole {
    pub(crate) fn new(
        swarm: Swarm<Behaviour>,
        membership: Membership,
        host: PeerId,
        core_receiver: tokio::sync::mpsc::UnboundedReceiver<OutgoingMessage>,
        message_sender: tokio::sync::mpsc::UnboundedSender<IncomingMessage>,
        chunk_routes: ChunkRoutes,
    ) -> Self {
        // A client starts out with nothing but the circuit to the host being
        // dialed, and is connected once that led to a direct connection and
        // its stream to the host is open on it.
        let session = Session::new(
            swarm,
            membership,
            host,
            HostLink::rendezvous(),
            core_receiver,
            message_sender,
            chunk_routes,
        );
        Self {
            session,
            host_reservation: None,
        }
    }

    fn on_ping(&mut self, event: ping::Event) {
        let host = self.session.net.host;
        if event.peer != host {
            return;
        }

        match event.result {
            // a circuit's round trip includes the detour over the relay
            Ok(rtt) if self.session.links.is_direct(&host, event.connection) => {
                self.session.forward(HostRoundTripMsg { rtt });
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "Ping to host failed, closing connection");
                self.session.net.swarm.close_connection(event.connection);
            }
        }
    }

    fn on_dcutr(&mut self, event: dcutr::Event) {
        let host = self.session.net.host;
        let peer = event.remote_peer_id;
        // A successful punch shows up as a direct connection, like any other
        // way of getting one.
        let Err(error) = event.result else {
            return;
        };
        warn!(%peer, %error, "Hole punching failed");
        // A punch is one of several ways to a direct connection (the host
        // also dials on the LAN), and a file-share peer is not needed for
        // the room. Without the host there is no room, though.
        if peer == host && !self.session.links.is_linked(&host) {
            self.session.net.stop(Stop::HostUnreachable);
        }
    }

    /// Joins the room: opens the stream to the host on the direct
    /// connection that exists now.
    fn on_host_linked(&mut self, addr: &Multiaddr) {
        self.session.net.host_link = HostLink::direct(addr);
        self.session.streams.join(self.session.net.host);
        self.reserve_host_circuit();
    }

    /// Reserves a slot on the host's relay, where other clients reach us
    /// for file-share hole punches. One reservation at a time: every
    /// `listen_on` of the circuit spawns a listener whose renewals count
    /// against the host relay's per-peer rate limit.
    fn reserve_host_circuit(&mut self) {
        let Some(host_circuit) = self.session.net.host_circuit() else {
            return;
        };
        match self.session.net.swarm.listen_on(host_circuit.clone()) {
            Ok(listener) => {
                info!(%host_circuit, "Reserving slot on host relay");
                self.host_reservation = Some(listener);
            }
            Err(err) => warn!(?err, %host_circuit, "Failed to listen on host relay circuit"),
        }
    }

    /// A reservation belongs to the connection it was made on, and its
    /// listener closes with it. While another direct connection keeps the
    /// host linked, the slot is reserved again there. A reservation the
    /// host's relay ended itself is not asked for again.
    fn on_listener_closed(&mut self, listener: ListenerId, reason: std::io::Result<()>) {
        if self.host_reservation != Some(listener) {
            return debug!(?listener, ?reason, "Listener closed");
        }
        self.host_reservation = None;
        let host = self.session.net.host;
        match reason {
            Ok(()) if self.session.links.is_linked(&host) => {
                info!("Reservation on host relay closed with its connection");
                self.reserve_host_circuit();
            }
            Ok(()) => debug!("Reservation on host relay closed with the link to the host"),
            Err(error) => warn!(%error, "Host relay ended the reservation"),
        }
    }

    fn on_outgoing_connection_error(
        &mut self,
        connection_id: ConnectionId,
        peer_id: Option<PeerId>,
        error: DialError,
    ) {
        let host = self.session.net.host;
        if peer_id == Some(host) && !self.session.links.is_connected(&host) {
            warn!(%error, %host, %connection_id, "Failed to reach the host");
            self.session.net.stop(Stop::HostUnreachable);
        } else {
            debug!(%error, ?peer_id, %connection_id, "Outgoing connection error");
        }
    }
}

#[async_trait]
impl Role for ClientRole {
    fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    fn on_opened(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
        opened: Opened,
    ) {
        let swarm = &mut self.session.net.swarm;
        match opened {
            Opened::Rendezvous => debug!(%peer_id, "Relay circuit established"),
            Opened::SpareCircuit => {
                debug!(%peer_id, "Closing relay circuit, direct connection already exists");
                swarm.close_connection(connection_id);
            }
            Opened::Linked { circuits } => {
                info!(%peer_id, ?endpoint, "Direct connection established");
                for circuit in circuits {
                    swarm.close_connection(circuit);
                }
                if peer_id == self.session.net.host {
                    self.on_host_linked(endpoint.get_remote_address());
                }
            }
            Opened::AnotherDirect => debug!(%peer_id, "Another direct connection established"),
        }
    }

    fn on_closed(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        cause: Option<ConnectionError>,
        closed: Closed,
    ) {
        if peer_id == self.session.net.host {
            let stop = match closed {
                Closed::Unlinked => Stop::HostLost,
                Closed::RendezvousLost => Stop::HostUnreachable,
                Closed::Unchanged => return,
            };
            warn!(?cause, %peer_id, %connection_id, "Connection to host closed");
            self.session.net.stop(stop);
        }
    }

    fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        use BehaviourEvent::*;
        use TransportBehaviourEvent as T;
        match event {
            SwarmEvent::Behaviour(Transport(T::Ping(e))) => self.on_ping(e),
            SwarmEvent::Behaviour(Transport(T::Dcutr(e))) => self.on_dcutr(e),
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                peer_id,
                error,
            } => self.on_outgoing_connection_error(connection_id, peer_id, error),
            SwarmEvent::ListenerClosed {
                listener_id,
                reason,
                ..
            } => self.on_listener_closed(listener_id, reason),
            other => debug!(event = ?other, "Received not captured event"),
        }
    }

    /// The client is connected once its stream to the host is open: the
    /// core hears of it before anything the host wrote, so it starts the
    /// connection afresh before it follows the room. When the stream ends
    /// the session ends as with a lost host, whatever the cause: the host
    /// turned the client down or dropped it, or a write did not get
    /// through. Whether the host applied what was written last is unknown
    /// then; the core connects anew and takes the room's state.
    fn on_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Opened(_) => self.session.forward(ConnectedMsg { is_host: false }),
            StreamEvent::Refused(_) => {
                warn!("Failed to open the stream to the host");
                self.session.net.stop(Stop::HostUnreachable);
            }
            StreamEvent::Received(_, NiketsuMessage::RoomState(msg)) => self.session.forward(msg),
            StreamEvent::Received(_, NiketsuMessage::Line(msg)) => self.session.forward(msg),
            StreamEvent::Received(_, other) => debug!(?other, "Not for the host to send"),
            StreamEvent::Ended(_) => {
                warn!("The stream to the host ended; leaving the room");
                self.session.net.stop(Stop::HostLost);
            }
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Option<OutgoingMessage> {
        let OutgoingMessage::Intent(intent) = msg else {
            return Some(msg);
        };
        self.session.streams.send(intent.into(), |_| true);
        None
    }
}

/// A client role against a host played by the test over the memory
/// transport.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::StreamExt;
    use libp2p::core::transport::MemoryTransport;
    use libp2p::core::upgrade::Version;
    use libp2p::multiaddr::Protocol;
    use libp2p::swarm::NetworkBehaviour;
    use libp2p::{Stream, Transport, noise, yamux};
    use libp2p_stream::IncomingStreams;
    use niketsu_core::chunk::ChunkRoute;
    use niketsu_core::communicator::{Intent, RoomStateMsg};
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::time::timeout;

    use super::*;
    use crate::p2p::member::MemberTag;
    use crate::p2p::stream::{PROTOCOL, read_frame, write_frame};

    const ROOM: &str = "room";
    const PASSWORD: &str = "secret";
    const PATIENCE: Duration = Duration::from_secs(5);

    /// What the host's side needs for the client to keep its connection:
    /// the room's streams, and pings.
    #[derive(NetworkBehaviour)]
    struct HostSide {
        streams: libp2p_stream::Behaviour,
        ping: ping::Behaviour,
    }

    fn swarm<B: NetworkBehaviour>(
        behaviour: impl FnOnce(&libp2p::identity::Keypair, libp2p::relay::client::Behaviour) -> B,
    ) -> Swarm<B> {
        libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_other_transport(|key| {
                Ok(MemoryTransport::default()
                    .upgrade(Version::V1)
                    .authenticate(noise::Config::new(key)?)
                    .multiplex(yamux::Config::default()))
            })
            .unwrap()
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .unwrap()
            .with_behaviour(|key, relay_client| behaviour(key, relay_client))
            .unwrap()
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build()
    }

    /// A client role dialing the test's host side, and the test's ends of
    /// the channels to the client's core.
    struct Setup {
        /// Where the client's stream arrives, if the host side takes the
        /// room's streams.
        arriving: Option<IncomingStreams>,
        host_membership: Membership,
        /// The client's peer id.
        peer: PeerId,
        core: tokio::sync::mpsc::UnboundedSender<OutgoingMessage>,
        to_core: tokio::sync::mpsc::UnboundedReceiver<IncomingMessage>,
        /// Why the client's session ended, once it has.
        run: tokio::task::JoinHandle<Stop>,
    }

    impl Setup {
        /// The client's stream once it presented the client's tag.
        async fn accept(&mut self) -> Stream {
            let arriving = self.arriving.as_mut().unwrap();
            let (from, mut stream) = timeout(PATIENCE, arriving.next())
                .await
                .expect("the client never opened its stream")
                .unwrap();
            assert_eq!(from, self.peer);
            let tag: MemberTag = timeout(PATIENCE, read_frame(&mut stream))
                .await
                .expect("the client never presented its tag")
                .unwrap();
            assert!(self.host_membership.admits(&self.peer, &tag));
            stream
        }

        /// What the core hears next beside the round trips the pings
        /// measure.
        async fn heard(&mut self) -> IncomingMessage {
            let next = async {
                loop {
                    match self.to_core.recv().await.unwrap() {
                        IncomingMessage::HostRoundTrip(_) => continue,
                        msg => return msg,
                    }
                }
            };
            timeout(PATIENCE, next)
                .await
                .expect("the core hears nothing")
        }
    }

    /// A client dialing a host side that takes the room's streams if
    /// `accepts`.
    async fn start(accepts: bool) -> Setup {
        let mut host_side = swarm(|_, _| HostSide {
            streams: libp2p_stream::Behaviour::new(),
            ping: ping::Behaviour::default(),
        });
        let host = *host_side.local_peer_id();
        host_side.listen_on("/memory/0".parse().unwrap()).unwrap();
        let addr = loop {
            if let SwarmEvent::NewListenAddr { address, .. } = host_side.select_next_some().await {
                break address.with(Protocol::P2p(host));
            }
        };
        let host_membership = Membership::new(ROOM.into(), PASSWORD.into(), &host);
        let mut control = host_side.behaviour_mut().streams.new_control();
        let arriving = accepts.then(|| control.accept(PROTOCOL).unwrap());
        tokio::spawn(async move {
            loop {
                host_side.select_next_some().await;
            }
        });

        let mut client =
            swarm(|key, relay_client| Behaviour::new(key, relay_client, false).unwrap());
        let peer = *client.local_peer_id();
        client.dial(addr).unwrap();
        let (core, core_receiver) = unbounded_channel();
        let (message_sender, to_core) = unbounded_channel();
        let routes = ChunkRoutes {
            fetch: ChunkRoute::default(),
            provide: ChunkRoute::default(),
        };
        let membership = Membership::new(ROOM.into(), PASSWORD.into(), &peer);
        let mut role = ClientRole::new(
            client,
            membership,
            host,
            core_receiver,
            message_sender,
            routes,
        );
        let run = tokio::spawn(async move { role.run().await });
        Setup {
            arriving,
            host_membership,
            peer,
            core,
            to_core,
            run,
        }
    }

    fn line(message: &str) -> Intent {
        Intent::Chat(message.into())
    }

    /// The client joins by opening its stream with its member tag, and its
    /// intents, chat lines among them, reach the host in the order the
    /// core sent them.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_client_joins_with_its_tag_and_its_intents_arrive_in_order() {
        let mut setup = start(true).await;
        let mut stream = setup.accept().await;
        let connected = setup.heard().await;
        assert!(
            matches!(
                connected,
                IncomingMessage::Connected(ConnectedMsg { is_host: false })
            ),
            "{connected:?}"
        );

        let sent = [
            Intent::Pause,
            line("one"),
            Intent::Speed(1.5),
            Intent::Start,
            line("two"),
        ];
        for intent in &sent {
            setup.core.send(intent.clone().into()).unwrap();
        }
        for intent in sent {
            let msg: NiketsuMessage = timeout(PATIENCE, read_frame(&mut stream))
                .await
                .expect("the client stopped sending")
                .unwrap();
            assert_eq!(msg, intent.into());
        }
    }

    /// The core hears of the connection before the host's first message,
    /// also when the host writes it the moment the stream arrives, so
    /// starting the connection afresh does not wipe what the host sent.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_core_hears_of_the_connection_before_the_hosts_first_message() {
        let mut setup = start(true).await;
        let mut stream = setup.accept().await;
        let state = RoomStateMsg {
            jumps: 3,
            ..Default::default()
        };
        let first = NiketsuMessage::RoomState(state.clone());
        write_frame(&mut stream, &first).await.unwrap();

        let connected = setup.heard().await;
        assert!(
            matches!(connected, IncomingMessage::Connected(_)),
            "the core first hears of the connection: {connected:?}"
        );
        let followed = setup.heard().await;
        assert!(
            matches!(&followed, IncomingMessage::RoomState(m) if *m == state),
            "then the host's first message: {followed:?}"
        );
    }

    /// When the host ends the stream, the client's session ends as with a
    /// lost host, and an intent the core sent meanwhile goes with it.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_client_leaves_a_host_that_ends_its_stream() {
        let mut setup = start(true).await;
        let stream = setup.accept().await;
        setup.heard().await;
        drop(stream);
        setup.core.send(Intent::Pause.into()).unwrap();
        let stop = timeout(PATIENCE, &mut setup.run).await;
        assert_eq!(stop.unwrap().unwrap(), Stop::HostLost);
    }

    /// A peer that takes no streams of the room hosts none: the client
    /// never is connected, and its session ends with an unreachable host.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_client_gives_up_on_a_peer_that_hosts_no_room() {
        let mut setup = start(false).await;
        let stop = timeout(PATIENCE, &mut setup.run).await;
        assert_eq!(stop.unwrap().unwrap(), Stop::HostUnreachable);
        while let Ok(msg) = setup.to_core.try_recv() {
            assert!(
                matches!(msg, IncomingMessage::HostRoundTrip(_)),
                "the core heard of a room: {msg:?}"
            );
        }
    }
}
