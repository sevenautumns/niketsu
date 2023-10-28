use config::Config;
use enum_dispatch::enum_dispatch;
use futures::future::OptionFuture;
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
        if self.model.config.auto_login {
            self.auto_login().await;
        }
        self.run_loop().await;
    }

    pub async fn auto_login(&mut self) {
        let addr = self.model.config.url.clone();
        let secure = self.model.config.secure;
        self.model
            .communicator
            .connect(EndpointInfo { addr, secure });
    }

    pub async fn run_loop(mut self) {
        let mut pacemaker = Pacemaker::default();
        loop {
            tokio::select! {
                com = self.model.communicator.receive() => {
                    com.handle(&mut self.model);
                }
                play = self.model.player.event() => {
                    play.handle(&mut self.model);
                }
                ui = self.model.ui.event() => {
                    ui.handle(&mut self.model);
                }
                beat = pacemaker.recv() => {
                    beat.handle(&mut self.model);
                }
                Some(db) = self.model.database.event() => {
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
