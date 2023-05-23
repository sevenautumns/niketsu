use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Error, Result};
use arc_swap::ArcSwap;
use enum_dispatch::enum_dispatch;
use log::{error, warn};
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::sync::Notify;

use self::heartbeat::Heartbeat;
use self::ui::UiMessage;
use crate::client::database::message::DatabaseMessage;
use crate::client::database::FileDatabase;
use crate::client::message::ClientMessage;
use crate::client::server::message::WebSocketMessage;
use crate::client::server::ServerWebsocket;
use crate::config::Config;
use crate::media_player::event::MediaPlayerEvent;
use crate::media_player::mpv::Mpv;
use crate::media_player::MediaPlayerWrapper;
use crate::messages::{MessagesReceiver, MessagesSender};
use crate::playlist::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::user::ThisUser;

pub mod database;
pub mod heartbeat;
pub mod message;
pub mod server;
pub mod ui;

#[derive(Debug)]
pub struct Client {
    changed: Arc<Notify>,
    sender: MpscSender<PlayerMessage>,
    db: Arc<FileDatabase>,
    ws: Arc<ArcSwap<ServerWebsocket>>,
    user: Arc<ArcSwap<ThisUser>>,
    messages: MessagesReceiver,
    playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    player: Arc<MediaPlayerWrapper<Mpv>>,
    rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

pub struct ClientInner {
    pub db: Arc<FileDatabase>,
    pub ws: Arc<ArcSwap<ServerWebsocket>>,
    pub config: Config,
    pub changed: Arc<Notify>,
    pub receiver: MpscReceiver<PlayerMessage>,
    pub user: Arc<ArcSwap<ThisUser>>,
    pub messages: MessagesSender,
    pub playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    pub player: Arc<MediaPlayerWrapper<Mpv>>,
    pub rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let db = Arc::new(FileDatabase::new(
            &config
                .media_dirs
                .iter()
                .map(|d| PathBuf::from_str(d).map_err(Error::msg))
                .collect::<Result<Vec<_>>>()?,
            tx.clone(),
        ));
        let db2 = db.clone();
        tokio::spawn(async move { db2.update().await });
        let player = Arc::new(MediaPlayerWrapper::new(tx.clone(), db.clone())?);
        let ws = Arc::new(ArcSwap::new(Arc::new(ServerWebsocket::new(
            config.addr()?,
            tx.clone(),
        ))));
        let user = Arc::new(ArcSwap::new(Arc::new(ThisUser::new(
            config.username.clone(),
        ))));
        let changed: Arc<Notify> = Default::default();
        let (msgs_tx, msgs_rx): (MessagesSender, MessagesReceiver) =
            crate::messages::messages_pair();
        let playlist_widget: Arc<ArcSwap<PlaylistWidgetState>> = Default::default();
        let rooms_widget: Arc<ArcSwap<RoomsWidgetState>> = Default::default();
        let inner = ClientInner {
            db: db.clone(),
            ws: ws.clone(),
            config,
            changed: changed.clone(),
            receiver: rx,
            player: player.clone(),
            user: user.clone(),
            messages: msgs_tx,
            playlist_widget: playlist_widget.clone(),
            rooms_widget: rooms_widget.clone(),
        };
        tokio::spawn(inner.run());
        Heartbeat::start(tx.clone());
        Ok(Self {
            changed,
            sender: tx,
            db,
            ws,
            user,
            messages: msgs_rx,
            playlist_widget,
            rooms_widget,
            player,
        })
    }

    pub fn player(&self) -> &MediaPlayerWrapper<Mpv> {
        &self.player
    }

    pub fn send_ui_message(&self, msg: UiMessage) {
        self.sender.send(msg.into()).map_err(Error::msg).log();
    }

    pub fn messages(&self) -> &MessagesReceiver {
        &self.messages
    }

    pub fn playlist(&self) -> Arc<ArcSwap<PlaylistWidgetState>> {
        self.playlist_widget.clone()
    }

    pub fn rooms(&self) -> Arc<ArcSwap<RoomsWidgetState>> {
        self.rooms_widget.clone()
    }

    pub fn user(&self) -> Arc<ArcSwap<ThisUser>> {
        self.user.clone()
    }

    pub fn db(&self) -> Arc<FileDatabase> {
        self.db.clone()
    }

    pub fn ws(&self) -> Arc<ServerWebsocket> {
        self.ws.load_full()
    }

    pub fn changed(&self) -> Arc<Notify> {
        self.changed.clone()
    }
}

impl ClientInner {
    pub async fn run(mut self) -> ! {
        loop {
            match self.receiver.recv().await {
                Some(msg) => msg.handle(&mut self).log(),
                None => {
                    error!("Inner loop receiver unexpectedly ended");
                    exit(1);
                }
            }
            self.changed.notify_waiters();
        }
    }
}

#[enum_dispatch(ClientMessage)]
#[derive(Debug, Clone)]
pub enum PlayerMessage {
    MediaPlayerEvent,
    Heartbeat,
    UiMessage,
    DatabaseMessage,
    WebSocketMessage,
}

pub trait LogResult {
    fn log(&self);
}

impl<T> LogResult for anyhow::Result<T> {
    fn log(&self) {
        if let Err(e) = self {
            warn!("{e:?}")
        }
    }
}
