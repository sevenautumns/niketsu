use std::borrow::Borrow;
use std::time::Duration;

use actix::{Handler, Message};

use super::actor::MpvActor;
use crate::client::server::{
    NiketsuPause, NiketsuPlaybackSpeed, NiketsuSeek, NiketsuSelect, NiketsuStart,
};
use crate::client::LogResult;
use crate::video::{PlayingFile, Video};

#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct MpvLoadFile {
    file: PlayingFile,
}

impl Handler<MpvLoadFile> for MpvActor {
    type Result = ();

    fn handle(&mut self, msg: MpvLoadFile, _: &mut Self::Context) -> Self::Result {
        self.load(msg.file).log()
    }
}

impl Handler<NiketsuPause> for MpvActor {
    type Result = ();

    fn handle(&mut self, _: NiketsuPause, _: &mut Self::Context) -> Self::Result {
        self.pause().log()
    }
}

impl Handler<NiketsuStart> for MpvActor {
    type Result = ();

    fn handle(&mut self, _: NiketsuStart, _: &mut Self::Context) -> Self::Result {
        self.start().log()
    }
}

impl Handler<NiketsuPlaybackSpeed> for MpvActor {
    type Result = ();

    fn handle(&mut self, msg: NiketsuPlaybackSpeed, _: &mut Self::Context) -> Self::Result {
        self.set_speed(msg.speed).log()
    }
}

impl Handler<NiketsuSeek> for MpvActor {
    type Result = ();

    fn handle(&mut self, msg: NiketsuSeek, _: &mut Self::Context) -> Self::Result {
        if !self.is_seeking().unwrap_or_default() {
            self.load(msg.borrow().into()).log();
        }
    }
}

impl Handler<NiketsuSelect> for MpvActor {
    type Result = ();

    fn handle(&mut self, msg: NiketsuSelect, _: &mut Self::Context) -> Self::Result {
        match msg.filename {
            Some(filename) => self
                .load(PlayingFile {
                    video: Video::from_string(filename),
                    paused: true,
                    speed: self.get_speed().expect("TODO replace this"),
                    pos: Duration::ZERO,
                })
                .log(),
            None => self.unload(),
        }
    }
}
