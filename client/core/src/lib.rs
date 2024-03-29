use config::Config;
use enum_dispatch::enum_dispatch;
use futures::future::OptionFuture;
use log::{info, trace};
use logging::ChatLogger;
use playlist::PlaylistHandlerTrait;

use self::communicator::*;
use self::file_database::*;
use self::heartbeat::Pacemaker;
use self::player::*;
use self::ui::*;

pub mod builder;
pub mod communicator;
pub mod config;
pub mod file_database;
pub mod heartbeat;
pub mod logging;
pub mod player;
pub mod playlist;
pub mod rooms;
pub mod ui;
pub mod user;
pub mod util;

#[enum_dispatch]
pub trait EventHandler {
    fn handle(self, model: &mut CoreModel);
}

#[derive(Debug)]
pub struct CoreModel {
    pub communicator: Box<dyn CommunicatorTrait>,
    pub player: Box<dyn MediaPlayerTrait>,
    pub ui: Box<dyn UserInterfaceTrait>,
    pub database: Box<dyn FileDatabaseTrait>,
    pub playlist: Box<dyn PlaylistHandlerTrait>,
    chat_logger: Option<ChatLogger>,
    pub config: Config,
    pub ready: bool,
}

#[derive(Debug)]
pub struct Core {
    pub model: CoreModel,
}

impl Core {
    pub async fn run(mut self) {
        info!("starting core");
        if self.model.config.auto_connect {
            info!("autoconnect to server");
            self.auto_connect().await;
        }
        self.run_loop().await;
    }

    pub async fn auto_connect(&mut self) {
        let addr = self.model.config.url.clone();
        let secure = self.model.config.secure;
        let endpoint = EndpointInfo { addr, secure };
        self.model.communicator.connect(endpoint);
    }

    pub async fn run_loop(mut self) {
        info!("enter main loop");
        let mut pacemaker = Pacemaker::default();
        loop {
            tokio::select! {
                com = self.model.communicator.receive() => {
                    trace!("handle communicator event");
                    com.handle(&mut self.model);
                }
                play = self.model.player.event() => {
                    trace!("handle player event");
                    play.handle(&mut self.model);
                }
                ui = self.model.ui.event() => {
                    trace!("handle ui event");
                    ui.handle(&mut self.model);
                }
                beat = pacemaker.recv() => {
                    trace!("handle pacemaker event");
                    beat.handle(&mut self.model);
                }
                Some(db) = self.model.database.event() => {
                    trace!("handle database event");
                    db.handle(&mut self.model);
                }
                Some(Some(message)) = OptionFuture::from(self.model.chat_logger.as_mut().map(|l| l.recv())) => {
                    self.model.ui.player_message(message)
                }
            }
        }
    }
}

#[macro_export]
macro_rules! log {
    ($result:expr) => {
        if let Err(err) = $result {
            log::error!("{:?}", err);
        }
    };
    ($result:expr, $default:expr) => {
        match $result {
            Ok(ok_val) => ok_val,
            Err(err) => {
                log::error!("{:?}", err);
                $default
            }
        }
    };
}
