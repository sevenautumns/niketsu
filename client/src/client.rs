use std::ops::Deref;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Error, Result};
use arc_swap::{ArcSwap, ArcSwapOption};
use log::{error, trace};
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::sync::Notify;

use crate::config::Config;
use crate::file_table::PlaylistWidgetState;
use crate::fs::{DatabaseMessage, FileDatabase};
use crate::heartbeat::Heartbeat;
use crate::messages::{MessagesReceiver, MessagesSender};
use crate::mpv::event::MpvEvent;
use crate::mpv::Mpv;
use crate::rooms::RoomsWidgetState;
use crate::user::ThisUser;
use crate::video::{PlayingFile, Video};
use crate::ws::{ServerWebsocket, WebSocketMessage};

#[derive(Debug)]
pub struct Client {
    changed: Arc<Notify>,
    sender: MpscSender<PlayerMessage>,
    db: Arc<FileDatabase>,
    ws: Arc<ArcSwap<ServerWebsocket>>,
    user: Arc<ArcSwap<ThisUser>>,
    messages: MessagesReceiver,
    playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    playing: Arc<ArcSwapOption<PlayingFile>>,
    rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

pub struct ClientInner {
    pub db: Arc<FileDatabase>,
    pub ws: Arc<ArcSwap<ServerWebsocket>>,
    pub mpv: Mpv,
    pub config: Config,
    pub changed: Arc<Notify>,
    pub receiver: MpscReceiver<PlayerMessage>,
    pub user: Arc<ArcSwap<ThisUser>>,
    pub messages: MessagesSender,
    pub playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    pub playing: Arc<ArcSwapOption<PlayingFile>>,
    pub rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut mpv = Mpv::new(tx.clone());
        mpv.init()?;
        let db = Arc::new(FileDatabase::new(
            &[PathBuf::from_str(&config.media_dir)?],
            tx.clone(),
        ));
        let db2 = db.clone();
        tokio::spawn(async move { db2.update().await });
        let ws = Arc::new(ArcSwap::new(Arc::new(ServerWebsocket::new(
            config.url.clone(),
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
        let playing: Arc<ArcSwapOption<PlayingFile>> = Default::default();
        let inner = ClientInner {
            db: db.clone(),
            ws: ws.clone(),
            config,
            changed: changed.clone(),
            receiver: rx,
            mpv,
            user: user.clone(),
            messages: msgs_tx,
            playlist_widget: playlist_widget.clone(),
            rooms_widget: rooms_widget.clone(),
            playing: playing.clone(),
        };
        trace!("Spawn inner loop");
        tokio::spawn(inner.run());
        trace!("Spawn heartbeat");
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
            playing,
        })
    }

    pub fn playing(&self) -> Option<PlayingFile> {
        self.playing
            .load()
            .deref()
            .as_ref()
            .map(|p| p.deref().clone())
    }

    pub fn send_ui_message(&self, msg: UiMessage) {
        self.sender
            .send(PlayerMessage::Ui(msg))
            .map_err(Error::msg)
            .log();
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
                Some(PlayerMessage::Mpv(event)) => self.react_to_mpv(event).log(),
                Some(PlayerMessage::Server(event)) => self.react_to_server(event).log(),
                Some(PlayerMessage::Database(event)) => self.react_to_database(event).log(),
                Some(PlayerMessage::Heartbeat) => self.react_to_heartbeat().log(),
                Some(PlayerMessage::Ui(UiMessage::MpvSelect(video))) => {
                    self.mpv.load(video, None, true, &self.db).log()
                }
                None => {
                    error!("Inner loop receiver unexpectedly ended");
                    exit(1);
                }
            }
            self.playing.store(self.mpv.playing().map(Arc::new));
            self.changed.notify_waiters();
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlayerMessage {
    Mpv(MpvEvent),
    Server(WebSocketMessage),
    Database(DatabaseMessage),
    Ui(UiMessage),
    Heartbeat,
}

pub trait LogResult {
    fn log(&self);
}

impl<T> LogResult for anyhow::Result<T> {
    fn log(&self) {
        if let Err(e) = self {
            error!("{e:?}")
        }
    }
}

#[derive(Debug, Clone)]
pub enum UiMessage {
    MpvSelect(Video),
}
