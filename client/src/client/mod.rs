use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Error, Result};
use arc_swap::{ArcSwap, ArcSwapOption};
use enum_dispatch::enum_dispatch;
use log::{error, warn};
use tokio::sync::mpsc::{UnboundedReceiver as MpscReceiver, UnboundedSender as MpscSender};
use tokio::sync::Notify;

use self::database::FileDatabaseSender;
use self::heartbeat::{Heartbeat, Pacemaker};
use self::server::ServerConnectionReceiver;
use self::ui::UiMessage;
use crate::client::database::message::DatabaseEvent;
use crate::client::database::FileDatabaseReceiver;
use crate::client::message::CoreMessageTrait;
use crate::client::server::message::WebSocketMessage;
use crate::client::server::ServerConnectionSender;
use crate::config::Config;
use crate::media_player::event::MediaPlayerEvent;
use crate::media_player::mpv::Mpv;
use crate::media_player::MediaPlayerWrapper;
use crate::messages::{MessagesReceiver, MessagesSender};
use crate::playlist::PlaylistWidgetState;
use crate::rooms::RoomsWidgetState;
use crate::user::ThisUser;
use crate::video::PlayingFile;

pub mod database;
pub mod heartbeat;
pub mod message;
pub mod server;
pub mod ui;

#[derive(Debug)]
pub struct Core {
    changed: Arc<Notify>,
    sender: MpscSender<UiMessage>,
    db: Arc<FileDatabaseSender>,
    ws: Arc<ArcSwap<ServerConnectionSender>>,
    user: Arc<ArcSwap<ThisUser>>,
    messages: MessagesReceiver,
    playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    playing_file: Arc<ArcSwapOption<PlayingFile>>,
    rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

pub struct CoreRunner {
    pub db: FileDatabaseReceiver,
    pub ws: ServerConnectionReceiver,
    pub ws_sender: Arc<ArcSwap<ServerConnectionSender>>,
    pub config: Config,
    pub changed: Arc<Notify>,
    pub receiver: MpscReceiver<UiMessage>,
    pub user: Arc<ArcSwap<ThisUser>>,
    pub messages: MessagesSender,
    pub pacemaker: Pacemaker,
    pub playlist_widget: Arc<ArcSwap<PlaylistWidgetState>>,
    pub player: MediaPlayerWrapper<Mpv>,
    pub playing_file: Arc<ArcSwapOption<PlayingFile>>,
    pub rooms_widget: Arc<ArcSwap<RoomsWidgetState>>,
}

impl Core {
    pub fn new(config: Config) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let db = FileDatabaseReceiver::new(
            &config
                .media_dirs
                .iter()
                .map(|d| PathBuf::from_str(d).map_err(Error::msg))
                .collect::<Result<Vec<_>>>()?,
        );
        FileDatabaseSender::start_update(db.sender());
        let db_sender = db.sender().clone();
        let player = MediaPlayerWrapper::new(db_sender.clone())?;
        let ws = ServerConnectionReceiver::new(config.addr()?);
        let user = Arc::new(ArcSwap::new(Arc::new(ThisUser::new(
            config.username.clone(),
        ))));
        let changed: Arc<Notify> = Default::default();
        let (msgs_tx, msgs_rx): (MessagesSender, MessagesReceiver) =
            crate::messages::messages_pair();
        let playlist_widget: Arc<ArcSwap<PlaylistWidgetState>> = Default::default();
        let rooms_widget: Arc<ArcSwap<RoomsWidgetState>> = Default::default();
        let playing_file: Arc<ArcSwapOption<PlayingFile>> = Default::default();
        let ws_sender = Arc::new(ArcSwap::new(Arc::new(ws.sender().clone())));
        let inner = CoreRunner {
            db,
            ws,
            ws_sender: ws_sender.clone(),
            config,
            changed: changed.clone(),
            receiver: rx,
            player,
            playing_file: playing_file.clone(),
            user: user.clone(),
            messages: msgs_tx,
            playlist_widget: playlist_widget.clone(),
            rooms_widget: rooms_widget.clone(),
            pacemaker: Default::default(),
        };
        tokio::spawn(inner.run());
        Ok(Self {
            changed,
            sender: tx,
            db: db_sender,
            ws: ws_sender,
            user,
            playing_file,
            messages: msgs_rx,
            playlist_widget,
            rooms_widget,
        })
    }

    pub fn send_ui_message(&self, msg: UiMessage) {
        self.sender.send(msg).map_err(Error::msg).log();
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

    pub fn playing_file(&self) -> Option<PlayingFile> {
        self.playing_file.load_full().map(|p| (*p).clone())
    }

    pub fn db(&self) -> Arc<FileDatabaseSender> {
        self.db.clone()
    }

    pub fn ws(&self) -> Arc<ServerConnectionSender> {
        self.ws.load().clone()
    }

    pub fn changed(&self) -> Arc<Notify> {
        self.changed.clone()
    }
}

impl CoreRunner {
    pub async fn run(mut self) -> ! {
        loop {
            match self.recv().await {
                Ok(msg) => msg.handle(&mut self).log(),
                Err(e) => error!("{e:?}"),
            }

            self.playing_file
                .store(self.player.playing_file().map(Arc::new));
            self.changed.notify_waiters();
        }
    }

    async fn recv(&mut self) -> Result<CoreMessage> {
        tokio::select! {
            p = self.player.recv() => p.map(CoreMessage::from),
            h = self.pacemaker.recv() => Ok(CoreMessage::from(h)),
            u = self.receiver.recv() => {
                let Some(u) = u else {
                    error!("UI receiver ended");
                    exit(1);
                };
                Ok(CoreMessage::from(u))
            }
            d = self.db.recv() => d.map(CoreMessage::from),
            w = self.ws.recv() => w.map(CoreMessage::from),
        }
    }
}

#[enum_dispatch(CoreMessageTrait)]
#[derive(Debug, Clone)]
pub enum CoreMessage {
    MediaPlayerEvent,
    Heartbeat,
    UiMessage,
    DatabaseEvent,
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
