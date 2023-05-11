use crate::ws::ServerMessage;

#[derive(Debug, Clone)]
pub struct ThisUser {
    name: String,
    ready: bool,
}

impl ThisUser {
    pub fn new(user: String) -> Self {
        ThisUser {
            name: user,
            ready: false,
        }
    }

    #[must_use]
    pub fn set_ready(&mut self, ready: bool) -> Option<ServerMessage> {
        if ready != self.ready {
            self.ready = ready;
            return Some(self.status());
        }
        None
    }

    pub fn toggle_ready(&mut self) {
        self.ready ^= true;
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn status(&self) -> ServerMessage {
        ServerMessage::Status {
            ready: self.ready,
            username: self.name(),
        }
    }

    #[must_use]
    pub fn set_name(&mut self, user: String) -> Option<ServerMessage> {
        if user.eq(&self.name) {
            self.name = user;
            return Some(self.status());
        }
        None
    }
}
