use actix::{Handler, Message};

use super::actor::User;

#[derive(Message)]
#[rtype(result = "()")]
pub enum UserReady {
    Ready,
    NotReady,
}

impl Handler<UserReady> for User {
    type Result = ();

    fn handle(&mut self, msg: UserReady, _: &mut Self::Context) -> Self::Result {
        let prev = self.ready;
        match msg {
            UserReady::Ready => self.ready = true,
            UserReady::NotReady => self.ready = false,
        }

        if prev != self.ready {
            self.send_state()
        }
    }
}
