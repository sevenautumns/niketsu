use std::sync::Arc;

use iced::Command;

use crate::window::MainMessage;
use crate::ws::ServerWebsocket;

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

    pub fn set_ready(&mut self, ready: bool, ws: &Arc<ServerWebsocket>) -> Command<MainMessage> {
        if ready != self.ready {
            self.ready = ready;
            return self.status(ws);
        }
        Command::none()
    }

    pub fn toggle_ready(&mut self, ws: &Arc<ServerWebsocket>) -> Command<MainMessage> {
        self.ready ^= true;
        self.status(ws)
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn status(&self, ws: &Arc<ServerWebsocket>) -> Command<MainMessage> {
        ServerWebsocket::send_command(
            ws,
            crate::ws::ServerMessage::Status {
                ready: self.ready,
                username: self.name(),
            },
        )
    }

    pub fn set_name(&mut self, user: String, ws: &Arc<ServerWebsocket>) -> Command<MainMessage> {
        if user.eq(&self.name) {
            self.name = user;
            return self.status(ws);
        }
        Command::none()
    }
}
