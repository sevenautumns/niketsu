//! Swarm-level tests for [`DirectOnly`]: three in-process swarms (relay, host,
//! dialer) over the memory transport, mirroring the production layout of an
//! un-wrapped relay client next to `DirectOnly`-wrapped messaging behaviours.
//!
//! The unit tests in [`super::direct_only`] feed hand-crafted `FromSwarm`
//! events into the wrapper, so they encode — rather than verify — what the
//! real swarm does. This test pins those assumptions against live libp2p
//! machinery:
//!
//! - relayed connections are recognized by `P2pCircuit` appearing in the
//!   addresses the swarm passes to `handle_established_*_connection`,
//! - the swarm-global `num_established` counts include the circuits hidden
//!   from the wrapped behaviour (the premise of the count adjustment),
//! - a peer that sends over the circuit anyway finds the protocol refused,
//! - request-response's connection bookkeeping and debug assertions survive
//!   a relayed-then-direct connection lifecycle, with substreams actually
//!   flowing through the `Either` handler plumbing in both directions.
//!
//! The same swarms pin what the handlers assume about relay reservations.

use std::time::Duration;

use futures::StreamExt;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::upgrade::Version;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundFailure, ProtocolSupport};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::{Multiaddr, StreamProtocol, Transport, noise, relay, yamux};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use super::direct_only::DirectOnly;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Serialize, Deserialize)]
struct Ping(u32);

#[derive(Debug, Serialize, Deserialize)]
struct Pong(u32);

/// Same shape as the production behaviour: hole-punch transport plumbing
/// (here just the relay client) un-wrapped, application protocols inside
/// [`DirectOnly`].
#[derive(NetworkBehaviour)]
struct TestBehaviour {
    relay_client: relay::client::Behaviour,
    messaging: DirectOnly<Messaging>,
}

/// A peer that runs its protocols over any connection, the circuit
/// included.
#[derive(NetworkBehaviour)]
struct Unwrapped {
    relay_client: relay::client::Behaviour,
    messaging: Messaging,
}

type Messaging = request_response::cbor::Behaviour<Ping, Pong>;

fn messaging() -> Messaging {
    request_response::cbor::Behaviour::new(
        [(
            StreamProtocol::new("/direct-only-test/1"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    )
}

fn swarm<B: NetworkBehaviour>(behaviour: impl FnOnce(relay::client::Behaviour) -> B) -> Swarm<B> {
    libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            Ok(MemoryTransport::default()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(key)?)
                .multiplex(yamux::Config::default()))
        })
        .expect("memory transport")
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .expect("relay client")
        .with_behaviour(|_, relay_client| behaviour(relay_client))
        .expect("behaviour")
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build()
}

fn client_swarm() -> Swarm<TestBehaviour> {
    swarm(|relay_client| TestBehaviour {
        relay_client,
        messaging: DirectOnly::new(messaging()),
    })
}

fn relay_swarm() -> Swarm<relay::Behaviour> {
    libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            Ok(MemoryTransport::default()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(key)?)
                .multiplex(yamux::Config::default()))
        })
        .expect("memory transport")
        .with_behaviour(|key| {
            relay::Behaviour::new(key.public().to_peer_id(), relay::Config::default())
        })
        .expect("behaviour")
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build()
}

/// Starts a listener and waits for its first reported address. For a circuit
/// address this blocks until the relay accepts the reservation.
async fn listen(swarm: &mut Swarm<impl NetworkBehaviour>, addr: Multiaddr) -> Multiaddr {
    let listener = swarm.listen_on(addr).expect("listen_on failed");
    timeout(TIMEOUT, async {
        loop {
            if let SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } = swarm.select_next_some().await
                && listener_id == listener
            {
                return address;
            }
        }
    })
    .await
    .expect("no listen address within timeout")
}

/// The next event of `swarm` that `find` picks.
async fn wait_for<B: NetworkBehaviour, T>(
    swarm: &mut Swarm<B>,
    what: &str,
    mut find: impl FnMut(SwarmEvent<B::ToSwarm>) -> Option<T>,
) -> T {
    timeout(TIMEOUT, async {
        loop {
            if let Some(found) = find(swarm.select_next_some().await) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never happened: {what}"))
}

/// What the client's handling of a closed reservation builds on (see
/// `ClientRole::on_listener_closed`): a reservation lives on
/// the oldest direct connection to the relay, its listener closes without
/// an error when that connection does, and the relay then accepts a
/// reservation over a connection that remains.
#[tokio::test]
async fn a_reservation_ends_with_its_connection_and_can_be_made_again() {
    let mut relay = relay_swarm();
    let relay_peer_id = *relay.local_peer_id();
    let relay_addr = listen(&mut relay, "/memory/0".parse().unwrap()).await;
    relay.add_external_address(relay_addr.clone());
    tokio::spawn(async move {
        loop {
            relay.select_next_some().await;
        }
    });

    let mut client = client_swarm();
    let mut connections = Vec::new();
    for _ in 0..2 {
        let dial = DialOpts::peer_id(relay_peer_id)
            .addresses(vec![relay_addr.clone()])
            .condition(PeerCondition::Always)
            .build();
        client.dial(dial).unwrap();
        let connection = wait_for(&mut client, "a connection to the relay", |e| match e {
            SwarmEvent::ConnectionEstablished { connection_id, .. } => Some(connection_id),
            _ => None,
        })
        .await;
        connections.push(connection);
    }

    let circuit = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    let reservation = client.listen_on(circuit.clone()).unwrap();
    wait_for(&mut client, "the relay accepts the reservation", |e| {
        matches!(e, SwarmEvent::NewListenAddr { listener_id, .. } if listener_id == reservation)
            .then_some(())
    })
    .await;

    assert!(client.close_connection(connections[0]));
    let reason = wait_for(
        &mut client,
        "the reservation's listener closes",
        |e| match e {
            SwarmEvent::ListenerClosed {
                listener_id,
                reason,
                ..
            } if listener_id == reservation => Some(reason),
            _ => None,
        },
    )
    .await;
    assert!(reason.is_ok(), "closed with {reason:?}");
    assert!(client.is_connected(&relay_peer_id));

    listen(&mut client, circuit).await;
}

#[tokio::test]
async fn protocols_are_gated_to_direct_connections_on_a_real_swarm() {
    let mut relay = relay_swarm();
    let relay_peer_id = *relay.local_peer_id();
    let relay_addr = listen(&mut relay, "/memory/0".parse().unwrap()).await;
    // The relay advertises its external addresses inside reservation vouchers;
    // without one, clients reject the reservation (`NoAddressesInReservation`).
    relay.add_external_address(relay_addr.clone());
    tokio::spawn(async move {
        loop {
            relay.select_next_some().await;
        }
    });

    let mut host = client_swarm();
    let host_peer_id = *host.local_peer_id();
    let host_direct_addr = listen(&mut host, "/memory/0".parse().unwrap()).await;
    let circuit_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    listen(&mut host, circuit_addr.clone()).await;
    let host_circuit = circuit_addr.with(Protocol::P2p(host_peer_id));

    let mut dialer = client_swarm();
    let dialer_peer_id = *dialer.local_peer_id();
    dialer.dial(host_circuit.clone()).unwrap();

    // Phase 1: only the relay circuit connects the two peers.
    timeout(TIMEOUT, async {
        let (mut dialer_connected, mut host_connected) = (false, false);
        while !(dialer_connected && host_connected) {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = event
                        && peer_id == host_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        dialer_connected = true;
                    }
                }
                event = host.select_next_some() => {
                    if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = event
                        && peer_id == dialer_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        host_connected = true;
                    }
                }
            }
        }
    })
    .await
    .expect("no relayed connection within timeout");

    // The circuit must be invisible to the wrapped behaviours: request-response
    // does not consider the peer connected, and a request parks (the dial it
    // issues aborts on its `Disconnected` condition, which `on_dial_failure`
    // ignores) instead of traveling over the relay. This holds both ways.
    for (side, peer, ping) in [
        (&mut dialer, host_peer_id, 7),
        (&mut host, dialer_peer_id, 8),
    ] {
        let messaging = side.behaviour_mut().messaging.inner_mut();
        assert!(!messaging.is_connected(&peer));
        messaging.send_request(&peer, Ping(ping));
    }

    // A peer that does not wrap its protocols sends over the circuit, and the
    // host refuses the protocol there.
    let mut unwrapped = swarm(|relay_client| Unwrapped {
        relay_client,
        messaging: messaging(),
    });
    let unwrapped_peer_id = *unwrapped.local_peer_id();
    unwrapped.dial(host_circuit).unwrap();
    let mut refused = false;
    timeout(TIMEOUT, async {
        while !refused {
            tokio::select! {
                event = unwrapped.select_next_some() => match event {
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. }
                        if peer_id == host_peer_id =>
                    {
                        assert!(endpoint.is_relayed());
                        let messaging = &mut unwrapped.behaviour_mut().messaging;
                        assert!(messaging.is_connected(&host_peer_id));
                        messaging.send_request(&host_peer_id, Ping(9));
                    }
                    SwarmEvent::Behaviour(UnwrappedEvent::Messaging(
                        request_response::Event::OutboundFailure { peer, error, .. },
                    )) => {
                        assert_eq!(peer, host_peer_id);
                        assert!(
                            matches!(error, OutboundFailure::UnsupportedProtocols),
                            "{error:?}"
                        );
                        refused = true;
                    }
                    SwarmEvent::Behaviour(UnwrappedEvent::Messaging(event)) => {
                        panic!("the host answered over the circuit: {event:?}")
                    }
                    _ => {}
                },
                event = host.select_next_some() => {
                    if let SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(event)) = event {
                        panic!("messaging activity over the relay circuit on the host: {event:?}");
                    }
                }
                _ = dialer.select_next_some() => {}
            }
        }
    })
    .await
    .expect("the host never refused the request over the circuit");
    drop(unwrapped);

    // Anything the wrapped behaviours report now ran over the circuit.
    let quiet = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(quiet);
    loop {
        tokio::select! {
            _ = &mut quiet => break,
            event = dialer.select_next_some() => {
                if let SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(event)) = event {
                    panic!("messaging activity over the relay circuit on the dialer: {event:?}");
                }
            }
            event = host.select_next_some() => match event {
                SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(event)) => {
                    panic!("messaging activity over the relay circuit on the host: {event:?}");
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    assert_eq!(peer_id, unwrapped_peer_id);
                }
                _ => {}
            }
        }
    }

    // Phase 2: a direct connection unblocks everything. The parked requests
    // drain onto it in both directions.
    dialer
        .dial(host_direct_addr.clone().with(Protocol::P2p(host_peer_id)))
        .unwrap();

    let mut direct_connection = None;
    let (mut dialer_pong, mut host_pong) = (None, None);
    // Answers a request, or returns the answer to one.
    let answer =
        |side: &mut Swarm<TestBehaviour>, event: SwarmEvent<TestBehaviourEvent>| -> Option<u32> {
            match event {
                SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                    request_response::Event::Message {
                        message:
                            request_response::Message::Request {
                                request, channel, ..
                            },
                        ..
                    },
                )) => {
                    let messaging = side.behaviour_mut().messaging.inner_mut();
                    messaging.send_response(channel, Pong(request.0)).unwrap();
                    None
                }
                SwarmEvent::Behaviour(TestBehaviourEvent::Messaging(
                    request_response::Event::Message {
                        message: request_response::Message::Response { response, .. },
                        ..
                    },
                )) => Some(response.0),
                _ => None,
            }
        };
    timeout(TIMEOUT, async {
        while direct_connection.is_none() || dialer_pong.is_none() || host_pong.is_none() {
            tokio::select! {
                event = dialer.select_next_some() => match event {
                    SwarmEvent::ConnectionEstablished {
                        peer_id, connection_id, endpoint, num_established, ..
                    } if peer_id == host_peer_id => {
                        assert!(!endpoint.is_relayed());
                        // The swarm-global count includes the circuit that
                        // `DirectOnly` hides from the wrapped behaviours.
                        assert_eq!(num_established.get(), 2);
                        direct_connection = Some(connection_id);
                    }
                    event => dialer_pong = answer(&mut dialer, event).or(dialer_pong),
                },
                event = host.select_next_some() => {
                    host_pong = answer(&mut host, event).or(host_pong);
                }
            }
        }
    })
    .await
    .expect("direct connection did not unblock messaging within timeout");
    assert_eq!((dialer_pong, host_pong), (Some(7), Some(8)));
    assert!(
        dialer
            .behaviour_mut()
            .messaging
            .inner_mut()
            .is_connected(&host_peer_id)
    );

    // Phase 3: closing the direct connection while the circuit is still open
    // delivers `ConnectionClosed` with a nonzero swarm-global count. The
    // wrapped behaviours must still run their full peer cleanup (adjusted
    // count 0); request-response debug_asserts its bookkeeping against exactly
    // this, so a miscount panics the test here.
    let direct_connection = direct_connection.unwrap();
    assert!(dialer.close_connection(direct_connection));
    timeout(TIMEOUT, async {
        let (mut dialer_closed, mut host_closed) = (false, false);
        while !(dialer_closed && host_closed) {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { connection_id, num_established, .. } = event
                        && connection_id == direct_connection
                    {
                        assert_eq!(num_established, 1, "the circuit should still be open");
                        dialer_closed = true;
                    }
                }
                event = host.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, .. } = event
                        && peer_id == dialer_peer_id
                    {
                        assert!(!endpoint.is_relayed());
                        assert_eq!(num_established, 1, "the circuit should still be open");
                        host_closed = true;
                    }
                }
            }
        }
    })
    .await
    .expect("direct connection did not close within timeout");
    assert!(
        !dialer
            .behaviour_mut()
            .messaging
            .inner_mut()
            .is_connected(&host_peer_id)
    );

    // Phase 4: the circuit closes last. Its `ConnectionClosed` must be
    // withheld from the wrapped behaviours — several assert on a matching
    // established/closed pairing for every connection they are told about.
    dialer.disconnect_peer_id(host_peer_id).unwrap();
    timeout(TIMEOUT, async {
        loop {
            tokio::select! {
                event = dialer.select_next_some() => {
                    if let SwarmEvent::ConnectionClosed { peer_id, endpoint, num_established, .. } = event
                        && peer_id == host_peer_id
                    {
                        assert!(endpoint.is_relayed());
                        assert_eq!(num_established, 0);
                        break;
                    }
                }
                _ = host.select_next_some() => {}
            }
        }
    })
    .await
    .expect("relay circuit did not close within timeout");
}
