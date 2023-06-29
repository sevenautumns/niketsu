use actix::{Actor, Context, Recipient};

use crate::client::server::{NiketsuMessage, NiketsuStatus};

#[derive(Debug, Clone)]
pub struct User {
    pub(super) name: String,
    pub(super) ready: bool,
    pub(super) server: Recipient<NiketsuMessage>,
}

impl Actor for User {
    type Context = Context<Self>;
}

impl User {
    pub(super) fn send_state(&mut self) {
        self.server.do_send(
            NiketsuStatus {
                ready: self.ready,
                username: self.name.clone(),
            }
            .into(),
        )
    }
}
