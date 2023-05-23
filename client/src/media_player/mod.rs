use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use parking_lot::RwLock;
use tokio::sync::mpsc::UnboundedSender as MpscSender;

use self::event::MediaPlayerEvent;
use self::mpv::MpvHandle;
use crate::client::database::FileDatabase;
use crate::client::PlayerMessage;
use crate::video::PlayingFile;

pub mod event;
pub mod mpv;

pub trait MediaPlayer: Sized {
    fn new(client_sender: MpscSender<PlayerMessage>) -> Result<Self>;
    fn pause(&self) -> Result<()>;
    fn is_paused(&self) -> Result<bool>;
    fn is_seeking(&self) -> Result<bool>;
    fn start(&self) -> Result<()>;
    fn set_speed(&self, speed: f64) -> Result<()>;
    fn get_speed(&self) -> Result<f64>;
    fn set_position(&self, pos: Duration) -> Result<()>;
    fn get_position(&self) -> Result<Duration>;
    fn open(&self, path: String, paused: bool, pos: Duration) -> Result<()>;
}

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerStatus {
    file: Option<PlayingFile>,
    file_loaded: bool,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerWrapper<M: MediaPlayer> {
    player: M,
    db: Arc<FileDatabase>,
    status: Arc<RwLock<MediaPlayerStatus>>,
}

impl<M: MediaPlayer> MediaPlayerWrapper<M> {
    pub fn new(client: MpscSender<PlayerMessage>, db: Arc<FileDatabase>) -> Result<Self> {
        let player = M::new(client)?;

        Ok(Self {
            player,
            status: Default::default(),
            db,
        })
    }

    pub fn pause(&self) -> Result<()> {
        let mut status = self.status.write();
        if let Some(file) = status.file.as_mut() {
            if !file.paused {
                file.paused = true;
                drop(status);
                return self.player.pause();
            }
        }
        Ok(())
    }

    pub fn is_paused(&self) -> Result<bool> {
        let file = self.status.read().file.clone();
        if let Some(file) = file {
            return Ok(file.paused);
        }
        self.player.is_paused()
    }

    pub fn start(&self) -> Result<()> {
        let mut status = self.status.write();
        if let Some(file) = status.file.as_mut() {
            if file.paused {
                file.paused = false;
                drop(status);
                return self.player.start();
            }
        }
        Ok(())
    }

    pub fn set_speed(&self, speed: f64) -> Result<()> {
        let mut status = self.status.write();
        if let Some(file) = status.file.as_mut() {
            if file.speed != speed {
                file.speed = speed;
                drop(status);
                return self.player.set_speed(speed);
            }
        }
        Ok(())
    }

    pub fn get_speed(&self) -> Result<f64> {
        let file = self.status.read().file.clone();
        if let Some(file) = file {
            return Ok(file.speed);
        }
        self.player.get_speed()
    }

    pub fn get_position(&self) -> Result<Duration> {
        self.player.get_position()
    }

    pub fn load(&self, play: PlayingFile) -> Result<()> {
        let file = self.status.read().file.clone();
        if let Some(file) = file {
            if file.video.as_str().eq(play.video.as_str()) {
                self.seek(play)
            } else {
                self.open(play)
            }
        } else {
            self.open(play)
        }
    }

    fn seek(&self, play: PlayingFile) -> Result<()> {
        let file = self.status.read().file.clone();
        if let Some(file) = file {
            if play.paused != file.paused {
                if play.paused {
                    self.pause()?;
                } else {
                    self.start()?;
                }
            }
            if play.speed != file.speed {
                self.set_speed(play.speed)?;
            }
            return self.player.set_position(play.pos);
        }
        Ok(())
    }

    fn open(&self, play: PlayingFile) -> Result<()> {
        let mut status = self.status.write();
        status.file = Some(play.clone());
        if let Some(path) = play.video.to_path_str(&self.db) {
            status.file_loaded = true;
            drop(status);
            self.player.open(path, play.paused, play.pos)
        } else {
            status.file_loaded = false;
            Ok(())
        }
    }

    pub fn reload(&self) -> Result<()> {
        let mut status = self.status.write();
        if !status.file_loaded {
            if let Some(file) = status.file.clone() {
                if let Some(path) = file.video.to_path_str(&self.db) {
                    status.file_loaded = true;
                    drop(status);
                    return self.player.open(path, file.paused, file.pos);
                }
            }
        }
        Ok(())
    }

    pub fn playing_file(&self) -> Option<PlayingFile> {
        let file = self.status.read().file.clone();
        if let Some(file) = file {
            return Some(file);
        }
        None
    }

    pub fn playing_file_mut<F>(&self, f: F)
    where
        F: FnOnce(&mut Option<PlayingFile>),
    {
        let mut status = self.status.write();
        f(&mut status.file)
    }

    pub fn is_seeking(&self) -> Result<bool> {
        self.player.is_seeking()
    }
}

// impl Mpv {
// pub fn reload(&mut self, db: &FileDatabase) -> Result<()> {
//     if let Some(playing) = &self.playing {
//         self.load(
//             playing.video.clone(),
//             playing.last_seek.pos(),
//             self.paused,
//             db,
//         )?;
//     }
//     Ok(())
// }

// pub fn may_reload(&mut self, db: &FileDatabase) -> Result<()> {
//     if let Some(PlayingFile {
//         video: Video::File { path, .. },
//         ..
//     }) = &self.playing
//     {
//         if path.is_none() {
//             return self.reload(db);
//         }
//     }
//     Ok(())
// }

// pub fn unload(&mut self) {
//     self.playing = None;
// }

// pub fn load(
//     &mut self,
//     mut video: Video,
//     seek: Option<Duration>,
//     paused: bool,
//     db: &FileDatabase,
// ) -> Result<()> {
//     let last_seek = seek.map(SeekEvent::new);
//     trace!("Set pause to {paused} during video load");
//     self.pause(paused)?;
//     let path = match &mut video {
//         Video::File { name, path } => {
//             let path = match path {
//                 Some(path) => path.clone(),
//                 None => {
//                     if let Ok(Some(file)) = db.find_file(name) {
//                         file.path
//                     } else {
//                         self.playing = Some(PlayingFile {
//                             video,
//                             last_seek,
//                             heartbeat: false,
//                         });
//                         return Ok(());
//                     }
//                 }
//             };
//             if let Some(path) = path.as_os_str().to_str() {
//                 CString::new(path)?
//             } else {
//                 bail!("Can not convert \"{path:?}\" to os_string")
//             }
//         }
//         Video::Url(url) => CString::new(url.as_str())?,
//     };

//     self.playing = Some(PlayingFile {
//         video,
//         last_seek,
//         heartbeat: false,
//     });

//     let cmd = MpvCommand::Loadfile.try_into()?;
//     self.send_command(&[&cmd, &path])?;
//     Ok(())
// }
// }
