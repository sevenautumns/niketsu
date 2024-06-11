use clap::Parser;

use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::prelude::*;
use futures::StreamExt;
use futures::{executor::block_on, future::FutureExt};
use libp2p::identity;
use libp2p::kad::store::MemoryStore;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::SwarmEvent;
use libp2p::{
    dcutr, gossipsub, identify, kad, noise, ping, relay, swarm::NetworkBehaviour, tcp, yamux,
    Multiaddr, PeerId,
};
use log::{debug, error, info, trace};
use std::collections::HashSet;
use std::error::Error;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::time::Duration;
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::spawn;
use uuid::fmt::Braced;
use uuid::Uuid;

use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::Swarm;

use bcrypt::{hash, verify, BcryptError, BcryptResult, DEFAULT_COST};
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, HashMap};

const RELAY_ADDRESS: &str =
    "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    file_request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
    init_request_response: request_response::cbor::Behaviour<InitRequest, InitResponse>,
}

impl Deref for Behaviour {
    type Target = kad::Behaviour<MemoryStore>;

    fn deref(&self) -> &Self::Target {
        &self.kademlia
    }
}

impl DerefMut for Behaviour {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kademlia
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    InboundRequest {
        request: String,
        channel: ResponseChannel<FileResponse>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileRequest(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileResponse(Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    intent: u8, // 0 -> listen, 1 -> dial
    // should be hashed when listening
    room: String,
    password: String,
}

impl InitRequest {
    fn new(intent: u8, room: String, password: String) -> Self {
        Self {
            intent,
            room,
            password: hash(password, DEFAULT_COST).expect("Failed to hash password"),
        }
    }

    fn new_without_hash(intent: u8, room: String, password: String) -> Self {
        Self {
            intent,
            room,
            password,
        }
    }

    fn verify(&self, password: String) -> bool {
        match verify(password, &self.password) {
            Err(e) => return false,
            Ok(valid) => return valid,
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitResponse {
    status: u8,              // 0 -> ok, 1 -> err
    peer_id: Option<PeerId>, // peer id of room if found
}

trait SwarmRelayConnection {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
        host: bool,
    ) -> Result<(), Box<dyn Error>>;
    async fn identify_loop(&mut self, room: String, password: String, host: bool)
        -> Option<PeerId>;
    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
        host: bool,
    ) -> Result<Option<PeerId>, Box<dyn Error>>;
}

impl SwarmRelayConnection for Swarm<Behaviour> {
    async fn establish_conection(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
        host: bool,
    ) -> Result<(), Box<dyn Error>> {
        self.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        self.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        let relay_peer_id = self
            .identify_relay(relay_addr.clone(), room, password, host)
            .await;

        match relay_peer_id {
            Ok(Some(peer_id)) => {
                info!("Dialing peer: {relay_peer_id:?}");
                self.dial(
                    relay_addr
                        .clone()
                        .with(Protocol::P2pCircuit)
                        .with(Protocol::P2p(peer_id)),
                )
                .unwrap();
                self.behaviour_mut()
                    .add_address(&peer_id, "/dnsaddr/bootstrap.libp2p.io".parse()?);
            }
            _ => {
                info!("Listening");
                self.listen_on(relay_addr.with(Protocol::P2pCircuit))
                    .expect("Failed to listen on remote relay");
            }
        };

        Ok(())
    }

    async fn identify_loop(
        &mut self,
        room: String,
        password: String,
        host: bool,
    ) -> Option<PeerId> {
        let mut relay_peer_id: Option<PeerId> = None;
        let mut learned_observed_addr = false;
        let mut told_relay_observed_addr = false;
        let mut observed_peer = false;

        loop {
            match self.next().await.unwrap() {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Sent {
                    ..
                })) => {
                    info!("Told relay its public address");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    peer_id,
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    info!("Relay told us our observed address: {observed_addr}");
                    if host {
                        info!("Sending new room creation request to relay");
                        self.behaviour_mut().init_request_response.send_request(
                            &peer_id,
                            InitRequest::new(0, room.clone(), password.clone()),
                        );
                    } else {
                        info!("Sending new room dialing request to relay");
                        self.behaviour_mut().init_request_response.send_request(
                            &peer_id,
                            InitRequest::new_without_hash(1, room.clone(), password.clone()),
                        );
                    }
                    relay_peer_id = Some(peer_id);
                    learned_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::InitRequestResponse(
                    request_response::Event::Message { peer, message, .. },
                )) => {
                    if let request_response::Message::Response { response, .. } = message {
                        relay_peer_id = response.peer_id;
                        observed_peer = true;
                    }
                }
                event => info!("Received other relay events: {event:?}"),
            }

            if learned_observed_addr && told_relay_observed_addr && observed_peer {
                break;
            }
        }

        relay_peer_id
    }

    async fn identify_relay(
        &mut self,
        relay_addr: Multiaddr,
        room: String,
        password: String,
        host: bool,
    ) -> Result<Option<PeerId>, Box<dyn Error>> {
        info!("Dialing relay for identify exchange");
        if let Err(e) = self.dial(relay_addr.clone()) {
            return Err("Failed to dial relay {relay_addr:?} with error {e:?}".into());
        }

        let room_peer_id = tokio::time::timeout(
            Duration::from_secs(15),
            self.identify_loop(room, password, host),
        )
        .await;

        match room_peer_id {
            Ok(pid) => return Ok(pid),
            Err(_) => Err("Identify exchange with relay timed out".into()),
        }
    }
}

pub(crate) struct P2PClient {
    sender: mpsc::Sender<Command>,
    receiver: mpsc::Receiver<Event>,
    relay_addr: Multiaddr,
    event_loop: Option<EventLoop>,
}

impl Deref for P2PClient {
    type Target = mpsc::Sender<Command>;

    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}

impl DerefMut for P2PClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sender
    }
}

pub(crate) trait DHTSender {
    async fn start_listening(&mut self, addr: Multiaddr) -> Result<(), Box<dyn Error + Send>>;
    async fn dial(
        &mut self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>>;
    async fn start_providing(&mut self, file_name: String);
    async fn get_providers(&mut self, file_name: String) -> HashSet<PeerId>;
    async fn request_file(
        &mut self,
        peer: PeerId,
        file_name: String,
    ) -> Result<Vec<u8>, Box<dyn Error + Send>>;
    async fn respond_file(&mut self, file: Vec<u8>, channel: ResponseChannel<FileResponse>);
}

impl DHTSender for mpsc::Sender<Command> {
    async fn start_listening(&mut self, addr: Multiaddr) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::StartListening { addr, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    async fn dial(
        &mut self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::Dial {
            peer_id,
            peer_addr,
            sender,
        })
        .await
        .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    async fn start_providing(&mut self, file_name: String) {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::StartProviding { file_name, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.");
    }

    /// Find the providers for the given file on the DHT.
    async fn get_providers(&mut self, file_name: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::GetProviders { file_name, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    async fn request_file(
        &mut self,
        peer: PeerId,
        file_name: String,
    ) -> Result<Vec<u8>, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.send(Command::RequestFile {
            file_name,
            peer,
            sender,
        })
        .await
        .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    async fn respond_file(&mut self, file: Vec<u8>, channel: ResponseChannel<FileResponse>) {
        self.send(Command::RespondFile { file, channel })
            .await
            .expect("Command receiver not to be dropped.");
    }
}

impl P2PClient {
    pub(crate) async fn new(
        room: String,
        password: String,
        host: bool,
    ) -> Result<P2PClient, Box<dyn Error>> {
        let key_pair = identity::Keypair::generate_ed25519();

        let (local_keys, peer_id) = if host {
            (key_pair, None)
        } else {
            let id = Uuid::new_v4();
            let keys = identity::Keypair::generate_ed25519();
            let peer_id = key_pair.public().to_peer_id();
            (keys, Some(peer_id))
        };

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_keys)
            .with_tokio()
            .with_tcp(
                tcp::Config::default().port_reuse(true).nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|keypair, relay_behaviour| {
                let message_id_fn = |_message: &gossipsub::Message| {
                    let id = Uuid::new_v4();
                    gossipsub::MessageId::from(id)
                };

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .duplicate_cache_time(Duration::from_secs(1))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .message_id_fn(message_id_fn)
                    .build()
                    .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?;

                let mut cfg = kad::Config::default();
                cfg.set_query_timeout(Duration::from_secs(5 * 60));

                Ok(Behaviour {
                    relay_client: relay_behaviour,
                    ping: ping::Behaviour::new(ping::Config::new()),
                    identify: identify::Behaviour::new(identify::Config::new(
                        "/identify/1".to_string(),
                        keypair.public(),
                    )),
                    dcutr: dcutr::Behaviour::new(keypair.public().to_peer_id()),
                    gossipsub: gossipsub::Behaviour::new(
                        gossipsub::MessageAuthenticity::Signed(keypair.clone()),
                        gossipsub_config,
                    )?,
                    kademlia: kad::Behaviour::with_config(
                        keypair.public().to_peer_id(),
                        kad::store::MemoryStore::new(keypair.public().to_peer_id()),
                        cfg,
                    ),
                    file_request_response: request_response::cbor::Behaviour::new(
                        [(StreamProtocol::new("/video-share/1"), ProtocolSupport::Full)],
                        request_response::Config::default(),
                    ),
                    init_request_response: request_response::cbor::Behaviour::new(
                        [(
                            StreamProtocol::new("/authorisation/1"),
                            ProtocolSupport::Full,
                        )],
                        request_response::Config::default(),
                    ),
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let relay_addr =
            Multiaddr::from_str(RELAY_ADDRESS).expect("Relay address could not be parsed");
        match swarm
            .establish_conection(relay_addr.clone(), room, password, host)
            .await
        {
            Err(e) => panic!("{e:?}"),
            Ok(_) => {}
        }

        let topic = gossipsub::IdentTopic::new("test-net");
        swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

        let (command_sender, command_receiver) = mpsc::channel(0);
        let (event_sender, event_receiver) = mpsc::channel(0);
        let client = P2PClient {
            sender: command_sender,
            receiver: event_receiver,
            relay_addr,
            event_loop: Some(EventLoop::new(swarm, topic, command_receiver, event_sender)),
        };

        Ok(client)
    }

    pub(crate) fn run(&mut self) -> tokio::task::JoinHandle<()> {
        let eventloop = self.event_loop.take().unwrap();
        spawn(eventloop.run())
    }

    pub(crate) async fn next_event(&mut self) -> Option<Event> {
        self.receiver.next().await
    }

    pub(crate) async fn get_file(&mut self, file_name: String) -> Result<Vec<u8>, Box<dyn Error>> {
        let providers = self.sender.get_providers(file_name.clone()).await;
        if providers.is_empty() {
            return Err(format!("Could not find provider for file {file_name}.").into());
        }

        // Request the content of the file from each node.
        let requests = providers.into_iter().map(|p| {
            let mut sender = self.sender.clone();
            let name = file_name.clone();
            async move { sender.request_file(p, name).await }.boxed()
        });

        let file_content = futures::future::select_ok(requests)
            .await
            .map_err(|_| "None of the providers returned file.")?
            .0;

        Ok(file_content)
    }
}

pub(crate) struct EventLoop {
    swarm: Swarm<Behaviour>,
    topic: gossipsub::IdentTopic,
    command_receiver: mpsc::Receiver<Command>,
    event_sender: mpsc::Sender<Event>,
    pending_dial: HashMap<PeerId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_start_providing: HashMap<kad::QueryId, oneshot::Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, oneshot::Sender<HashSet<PeerId>>>,
    pending_request_file:
        HashMap<OutboundRequestId, oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>>,
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        topic: gossipsub::IdentTopic,
        command_receiver: mpsc::Receiver<Command>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            swarm,
            topic,
            command_receiver,
            event_sender,
            pending_dial: Default::default(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_file: Default::default(),
        }
    }

    pub(crate) async fn run(mut self) {
        let mut stdin = io::BufReader::new(io::stdin()).lines();
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                command = self.command_receiver.next() => match command {
                    Some(c) => {
                        info!("{c:?}");
                        self.handle_command(c).await},
                    None => {}, // channel closed
                },
                Ok(Some(line)) = stdin.next_line() => {
                    info!("YEAH!");
                    if let Err(e) = self.swarm
                        .behaviour_mut().gossipsub
                        .publish(self.topic.clone(), line.as_bytes()) {
                     println!("Publish error: {e:?}");
                    }
                }
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => info!(
                "Got message: '{}'\n with id: {id} from peer: {peer_id}",
                String::from_utf8_lossy(&message.data),
            ),
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result: kad::QueryResult::StartProviding(_),
                    ..
                },
            )) => {
                let sender: oneshot::Sender<()> = self
                    .pending_start_providing
                    .remove(&id)
                    .expect("Completed query to be previously pending.");
                let _ = sender.send(());
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result:
                        kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {
                            providers,
                            ..
                        })),
                    ..
                },
            )) => {
                if let Some(sender) = self.pending_get_providers.remove(&id) {
                    sender.send(providers).expect("Receiver not to be dropped");

                    // Finish the query. We are only interested in the first result.
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .query_mut(&id)
                        .unwrap()
                        .finish();
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    result:
                        kad::QueryResult::GetProviders(Ok(
                            kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. },
                        )),
                    ..
                },
            )) => {}
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => {
                info!("{event:?}")
            }
            SwarmEvent::Behaviour(BehaviourEvent::FileRequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    self.event_sender
                        .send(Event::InboundRequest {
                            request: request.0,
                            channel,
                        })
                        .await
                        .expect("Event receiver not to be dropped.");
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    let _ = self
                        .pending_request_file
                        .remove(&request_id)
                        .expect("Request to still be pending.")
                        .send(Ok(response.0));
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::FileRequestResponse(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                let _ = self
                    .pending_request_file
                    .remove(&request_id)
                    .expect("Request to still be pending.")
                    .send(Err(Box::new(error)));
            }
            SwarmEvent::Behaviour(BehaviourEvent::FileRequestResponse(
                request_response::Event::ResponseSent { .. },
            )) => {}
            SwarmEvent::NewListenAddr { address, .. } => {
                let local_peer_id = *self.swarm.local_peer_id();
                info!(
                    "Local node is listening on {:?}",
                    address.with(Protocol::P2p(local_peer_id))
                );
            }
            SwarmEvent::IncomingConnection { .. } => {}
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                if endpoint.is_dialer() {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Ok(()));
                    }
                }
            }
            SwarmEvent::ConnectionClosed { .. } => {}
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                if let Some(peer_id) = peer_id {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Err(Box::new(error)));
                    }
                }
            }
            SwarmEvent::IncomingConnectionError { .. } => {}
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => info!("Dialing {peer_id}"),
            e => info!("{e:?}"),
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::StartListening { addr, sender } => {
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(Box::new(e))),
                };
            }
            Command::Dial {
                peer_id,
                peer_addr,
                sender,
            } => {
                if let hash_map::Entry::Vacant(e) = self.pending_dial.entry(peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, peer_addr.clone());
                    match self.swarm.dial(peer_addr.with(Protocol::P2p(peer_id))) {
                        Ok(()) => {
                            e.insert(sender);
                        }
                        Err(e) => {
                            let _ = sender.send(Err(Box::new(e)));
                        }
                    }
                } else {
                    todo!("Already dialing peer.");
                }
            }
            Command::StartProviding { file_name, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(file_name.into_bytes().into())
                    .expect("No store error.");
                self.pending_start_providing.insert(query_id, sender);
            }
            Command::GetProviders { file_name, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(file_name.into_bytes().into());
                self.pending_get_providers.insert(query_id, sender);
            }
            Command::RequestFile {
                file_name,
                peer,
                sender,
            } => {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .file_request_response
                    .send_request(&peer, FileRequest(file_name));
                self.pending_request_file.insert(request_id, sender);
            }
            Command::RespondFile { file, channel } => {
                self.swarm
                    .behaviour_mut()
                    .file_request_response
                    .send_response(channel, FileResponse(file))
                    .expect("Connection to peer to be still open.");
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProviding {
        file_name: String,
        sender: oneshot::Sender<()>,
    },
    GetProviders {
        file_name: String,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    RequestFile {
        file_name: String,
        peer: PeerId,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    RespondFile {
        file: Vec<u8>,
        channel: ResponseChannel<FileResponse>,
    },
}
