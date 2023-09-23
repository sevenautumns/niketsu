use enum_dispatch::enum_dispatch;

use self::communicator::*;
use self::file_database::*;
use self::heartbeat::Pacemaker;
use self::player::*;
use self::ui::*;
use self::user::UserStatus;
use crate::playlist::PlaylistHandler;

pub mod build;
pub mod communicator;
pub mod file_database;
pub mod heartbeat;
pub mod player;
pub mod playlist;
pub mod ui;
pub mod user;

#[enum_dispatch]
pub trait EventHandler {
    fn handle(self, model: &mut CoreModel);
}

#[derive(Debug)]
pub struct CoreModel {
    pub communicator: Box<dyn CommunicatorTrait>,
    pub database: Box<dyn FileDatabaseTrait>,
    pub player: Box<dyn MediaPlayerTrait>,
    pub ui: Box<dyn UserInterfaceTrait>,
    pub playlist: PlaylistHandler,
    // TODO put the following in their own struct?
    pub user: UserStatus,
    pub room: String,
    pub password: Option<String>,
}

#[derive(Debug)]
pub struct Core {
    model: CoreModel,
}

impl Core {
    pub async fn run(mut self) {
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
            }
        }
    }
}
