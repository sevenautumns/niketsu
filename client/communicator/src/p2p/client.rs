use anyhow::Result;
use async_trait::async_trait;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::request_response::{self, OutboundRequestId, ResponseChannel};
use libp2p::swarm::{ConnectionError, ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId, dcutr, ping};
use niketsu_core::communicator::{
    ConnectedMsg, HostRoundTripMsg, IncomingMessage, OutgoingMessage,
};
use niketsu_core::log_err_msg;
use tracing::{debug, info, warn};

use super::links::{Closed, Opened};
use super::member::Membership;
use super::outbox::Outbox;
use super::{
    Behaviour, BehaviourEvent, ChunkRoutes, HostLink, MessageResponse, Role, Session,
    StatusResponse, Stop, TransportBehaviourEvent,
};
use crate::messages::NiketsuMessage;

pub(crate) struct ClientRole {
    session: Session,
    /// The listener on the host's circuit, which lives as long as the
    /// reservation on the host's relay does.
    host_reservation: Option<ListenerId>,
    /// The request to join the room, until the host answers it.
    join: Option<OutboundRequestId>,
    /// What this peer sends the host, its intents, which reach the host in
    /// the order sent.
    outbox: Outbox<OutboundRequestId>,
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
        // the host took it into the room.
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
            join: None,
            outbox: Outbox::default(),
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
                if let Err(error) = self.session.forward(HostRoundTripMsg { rtt }) {
                    warn!(%error, "Failed to report round trip to core");
                }
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

    /// Asks the host to take this peer into the room.
    fn on_host_linked(&mut self, addr: &Multiaddr) {
        let net = &mut self.session.net;
        net.host_link = HostLink::direct(addr);
        let host = net.host;
        self.join = Some(net.send_request(&host, NiketsuMessage::Join));
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

    /// The host did not take the message on its way to it: it refused it,
    /// the request timed out or the connection closed. Whether the host
    /// applied it is unknown, and what waits behind it would reach the
    /// host out of step, so the session ends as with a lost host, and
    /// what waits goes with it. The core connects anew and is greeted with
    /// the room.
    fn lose_host(&mut self, why: &dyn std::fmt::Debug) {
        warn!(?why, "The host did not take a message; leaving the room");
        self.session.net.stop(Stop::HostLost);
    }

    /// The host took this peer into the room: it answered the request to
    /// join, or sent the room's first message, which can arrive before
    /// that answer. The client is connected from here on. The core learns
    /// of it here: after the host took this peer in, so the core's first
    /// requests reach a member, and before the host's first message, so
    /// the core starts the connection afresh before it follows the room.
    fn joined(&mut self) {
        if self.join.take().is_none() {
            return;
        }
        let res = self.session.forward(ConnectedMsg { is_host: false });
        log_err_msg!(res, "Failed to send connected message to core");
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

    fn handle_swarm_request(
        &mut self,
        msg: NiketsuMessage,
        channel: ResponseChannel<MessageResponse>,
        peer_id: PeerId,
    ) -> Result<()> {
        debug!("Received swarm request {msg:?}");
        // only the host speaks for the room, and only to its members
        if peer_id != self.session.net.host {
            return self.session.respond_with_err(msg, channel);
        }
        self.joined();
        use NiketsuMessage::*;
        match msg {
            RoomState(m) => self.session.send_to_core(m, channel),
            Line(m) => self.session.send_to_core(m, channel),
            other => self.session.respond_with_err(other, channel),
        }
    }

    fn on_msg_response(
        &mut self,
        peer: PeerId,
        request_id: OutboundRequestId,
        response: MessageResponse,
    ) {
        debug!(?response, %peer, "Received response");
        if self.join != Some(request_id) {
            let net = &mut self.session.net;
            let host = net.host;
            match response.0 {
                StatusResponse::Ok => {
                    self.outbox
                        .answered(&request_id, |msg| net.send_request(&host, msg));
                }
                StatusResponse::Err if self.outbox.is_on_its_way(&request_id) => {
                    self.lose_host(&response)
                }
                StatusResponse::Err => {}
            }
            return;
        }
        match response.0 {
            StatusResponse::Ok => self.joined(),
            StatusResponse::Err => {
                self.join = None;
                warn!(%peer, "The host refused to take this peer into the room");
                self.session.net.stop(Stop::HostUnreachable);
            }
        }
    }

    fn on_msg_req_resp_other(
        &mut self,
        event: request_response::Event<super::MessageRequest, MessageResponse>,
    ) {
        match event {
            request_response::Event::OutboundFailure {
                peer,
                request_id,
                error,
                ..
            } => {
                let host = self.session.net.host;
                if self.join == Some(request_id) {
                    warn!(%error, %host, "Failed to join the room");
                    self.session.net.stop(Stop::HostUnreachable);
                } else if peer == host && self.outbox.is_on_its_way(&request_id) {
                    self.lose_host(&error);
                }
            }
            other => debug!(
                ?other,
                "Received request response event that is not handled"
            ),
        }
    }

    fn handle_core_message(&mut self, msg: OutgoingMessage) -> Option<OutgoingMessage> {
        let OutgoingMessage::Intent(intent) = msg else {
            return Some(msg);
        };
        let net = &mut self.session.net;
        let host = net.host;
        self.outbox
            .push(intent.into(), |msg| net.send_request(&host, msg));
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
    use libp2p::request_response::{Event, Message};
    use libp2p::swarm::NetworkBehaviour;
    use libp2p::{Transport, noise, yamux};
    use niketsu_core::chunk::ChunkRoute;
    use niketsu_core::communicator::{Intent, RoomStateMsg};
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;
    use crate::p2p::{MessagingBehaviour, messaging};

    const ROOM: &str = "room";
    const PASSWORD: &str = "secret";

    /// What the host's side needs for the client to keep its connection:
    /// the room's messages, and pings.
    #[derive(NetworkBehaviour)]
    struct HostSide {
        messaging: MessagingBehaviour,
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
        host_side: Swarm<HostSide>,
        host_membership: Membership,
        /// The client's peer id.
        peer: PeerId,
        core: tokio::sync::mpsc::UnboundedSender<OutgoingMessage>,
        to_core: tokio::sync::mpsc::UnboundedReceiver<IncomingMessage>,
        /// Why the client's session ended, once it has.
        run: tokio::task::JoinHandle<Stop>,
    }

    async fn start() -> Setup {
        let mut host_side = swarm(|_, _| HostSide {
            messaging: messaging(),
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
            host_side,
            host_membership,
            peer,
            core,
            to_core,
            run,
        }
    }

    /// The next room message the client sends the host side.
    async fn next_request(
        host_side: &mut Swarm<HostSide>,
    ) -> (NiketsuMessage, ResponseChannel<MessageResponse>) {
        loop {
            if let SwarmEvent::Behaviour(HostSideEvent::Messaging(Event::Message {
                message:
                    Message::Request {
                        request, channel, ..
                    },
                ..
            })) = host_side.select_next_some().await
            {
                return (request.msg, channel);
            }
        }
    }

    /// Whether the client sends the host side nothing for a while.
    async fn sends_nothing(host_side: &mut Swarm<HostSide>) -> bool {
        let wait = Duration::from_millis(300);
        tokio::time::timeout(wait, next_request(host_side))
            .await
            .is_err()
    }

    /// The host side takes the client into the room.
    async fn join(setup: &mut Setup) {
        let patience = Duration::from_secs(5);
        let (msg, channel) = tokio::time::timeout(patience, next_request(&mut setup.host_side))
            .await
            .expect("the client never asked to join");
        assert_eq!(msg, NiketsuMessage::Join);
        let messaging = &mut setup.host_side.behaviour_mut().messaging;
        let ok = MessageResponse(StatusResponse::Ok);
        messaging.send_response(channel, ok).unwrap();
    }

    fn line(message: &str) -> Intent {
        Intent::Chat(message.into())
    }

    /// The client's intents, chat lines among them, reach the host in the
    /// order the core sent them, one at a time: the next goes out once the
    /// host answered the one before.
    #[tokio::test(flavor = "multi_thread")]
    async fn intents_and_chat_reach_the_host_in_the_order_sent() {
        let mut setup = start().await;
        join(&mut setup).await;

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
        let sent: Vec<NiketsuMessage> = sent.into_iter().map(NiketsuMessage::from).collect();

        let mut received = Vec::new();
        for _ in &sent {
            let patience = Duration::from_secs(5);
            let next = next_request(&mut setup.host_side);
            let (msg, channel) = tokio::time::timeout(patience, next)
                .await
                .expect("the client stopped sending");
            received.push(msg);
            assert!(
                sends_nothing(&mut setup.host_side).await,
                "a second message while one is unanswered"
            );
            let messaging = &mut setup.host_side.behaviour_mut().messaging;
            let ok = MessageResponse(StatusResponse::Ok);
            messaging.send_response(channel, ok).unwrap();
        }
        assert_eq!(received, sent);
    }

    /// A message the host does not take ends the client's session as a
    /// lost host, and what waited behind it goes with the session: a
    /// refusal, and a request the host drops unanswered.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_client_leaves_a_host_that_does_not_take_its_message() {
        for refuses in [true, false] {
            let mut setup = start().await;
            join(&mut setup).await;
            setup.core.send(Intent::Pause.into()).unwrap();
            setup.core.send(line("behind it").into()).unwrap();

            let patience = Duration::from_secs(5);
            let next = next_request(&mut setup.host_side);
            let (msg, channel) = tokio::time::timeout(patience, next).await.unwrap();
            assert_eq!(msg, Intent::Pause.into());
            if refuses {
                let messaging = &mut setup.host_side.behaviour_mut().messaging;
                let refusal = MessageResponse(StatusResponse::Err);
                messaging.send_response(channel, refusal).unwrap();
            } else {
                drop(channel);
            }
            let ended = async {
                tokio::select! {
                    stop = &mut setup.run => stop.unwrap(),
                    _ = next_request(&mut setup.host_side) => panic!("the next message went out"),
                }
            };
            let stop = tokio::time::timeout(patience, ended).await;
            assert_eq!(stop.ok(), Some(Stop::HostLost), "refuses: {refuses}");
        }
    }

    /// The host's first message can overtake its answer to the request to
    /// join. The core hears of the connection before that message all the
    /// same, so starting the connection afresh does not wipe what the host
    /// sent.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_core_hears_of_the_connection_before_the_hosts_first_message() {
        let Setup {
            mut host_side,
            host_membership,
            peer,
            core: _core,
            mut to_core,
            ..
        } = start().await;

        // the host answers the request to join only once the client took
        // the room's first message
        let state = RoomStateMsg {
            jumps: 3,
            ..Default::default()
        };
        let mut join = None;
        let mut first = None;
        let serve = async {
            loop {
                let SwarmEvent::Behaviour(HostSideEvent::Messaging(event)) =
                    host_side.select_next_some().await
                else {
                    continue;
                };
                let messaging = &mut host_side.behaviour_mut().messaging;
                match event {
                    Event::Message {
                        message:
                            Message::Request {
                                request, channel, ..
                            },
                        ..
                    } if request.msg == NiketsuMessage::Join => {
                        join = Some(channel);
                        let msg = host_membership.tag(NiketsuMessage::RoomState(state.clone()));
                        first = Some(messaging.send_request(&peer, msg));
                    }
                    Event::Message {
                        message: Message::Response { request_id, .. },
                        ..
                    } if Some(request_id) == first => {
                        let ok = MessageResponse(StatusResponse::Ok);
                        messaging.send_response(join.take().unwrap(), ok).unwrap();
                        return;
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), serve)
            .await
            .expect("the client never took the host's first message");

        // what the core hears beside the round trips the pings measure
        let mut next = async || loop {
            match to_core.recv().await.unwrap() {
                IncomingMessage::HostRoundTrip(_) => continue,
                msg => return msg,
            }
        };
        let connected = tokio::time::timeout(Duration::from_secs(5), next()).await;
        assert!(
            matches!(
                connected,
                Ok(IncomingMessage::Connected(ConnectedMsg {
                    is_host: false,
                    ..
                }))
            ),
            "the core first hears of the connection: {connected:?}"
        );
        let followed = tokio::time::timeout(Duration::from_secs(5), next()).await;
        assert!(
            matches!(&followed, Ok(IncomingMessage::RoomState(m)) if *m == state),
            "then the host's first message: {followed:?}"
        );
    }
}
