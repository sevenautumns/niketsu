use actix::{Actor, AsyncContext, Context, Recipient};
use anymap::AnyMap;

use super::MediaPlayer;
use crate::client::server::NiketsuMessage;
use crate::file_system::actor::FileDatabaseModel;
use crate::video::PlayingFile;

#[derive(Debug)]
pub struct Player<M: MediaPlayer, F: FileDatabaseModel> {
    pub(super) player: M,
    pub(super) db: F,
    pub(super) file: Option<PlayingFile>,
    pub(super) file_loaded: bool,
    pub(super) subscriber: AnyMap,
    // pub(super) server: Recipient<NiketsuMessage>,
}

impl<M: MediaPlayer, F: FileDatabaseModel> Actor for Player<M, F> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        ctx.spawn((|| {
            // addr.recipient();
            actix::fut::ready(())
        })());
    }
}
