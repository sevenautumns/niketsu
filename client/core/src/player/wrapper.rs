use std::ops::{Deref, DerefMut};
use std::time::Duration;

use super::MediaPlayerTrait;

#[derive(Debug)]
pub struct MediaPlayer {
    player: Box<dyn MediaPlayerTrait>,
}

impl MediaPlayer {
    pub fn new(player: Box<dyn MediaPlayerTrait>) -> Self {
        Self { player }
    }

    pub fn versöhnen(&mut self, pos: Duration) {
        todo!()
    }
}

impl Deref for MediaPlayer {
    type Target = Box<dyn MediaPlayerTrait>;

    fn deref(&self) -> &Self::Target {
        &self.player
    }
}

impl DerefMut for MediaPlayer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.player
    }
}
