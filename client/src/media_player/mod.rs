use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use self::event::MediaPlayerEvent;
use self::mpv::MpvHandle;
use crate::client::database::FileDatabaseSender;
use crate::video::PlayingFile;

pub mod event;
pub mod mpv;

#[async_trait]
pub trait MediaPlayer: Sized {
    fn new() -> Result<Self>;
    fn pause(&mut self) -> Result<()>;
    fn is_paused(&self) -> Result<bool>;
    fn is_seeking(&self) -> Result<bool>;
    fn start(&mut self) -> Result<()>;
    fn set_speed(&mut self, speed: f64) -> Result<()>;
    fn get_speed(&self) -> Result<f64>;
    fn set_position(&mut self, pos: Duration) -> Result<()>;
    fn get_position(&self) -> Result<Duration>;
    fn open(&self, path: String, paused: bool, pos: Duration) -> Result<()>;
    #[must_use]
    async fn receive_event(&mut self) -> Result<MediaPlayerEvent>;
}

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerStatus {
    file: Option<PlayingFile>,
    file_loaded: bool,
}

#[derive(Debug)]
pub struct MediaPlayerWrapper<M: MediaPlayer> {
    player: M,
    db: Arc<FileDatabaseSender>,
    status: MediaPlayerStatus,
}

impl<M: MediaPlayer> MediaPlayerWrapper<M> {
    pub fn new(db: Arc<FileDatabaseSender>) -> Result<Self> {
        let player = M::new()?;

        Ok(Self {
            player,
            status: Default::default(),
            db,
        })
    }

    pub fn pause(&mut self) -> Result<()> {
        if let Some(file) = self.status.file.as_mut() {
            if !file.paused {
                file.paused = true;
                return self.player.pause();
            }
        }
        Ok(())
    }

    pub fn is_paused(&self) -> Result<bool> {
        if let Some(file) = &self.status.file {
            return Ok(file.paused);
        }
        self.player.is_paused()
    }

    pub fn start(&mut self) -> Result<()> {
        if let Some(file) = self.status.file.as_mut() {
            if file.paused {
                file.paused = false;
                return self.player.start();
            }
        }
        Ok(())
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<()> {
        if let Some(file) = self.status.file.as_mut() {
            if file.speed != speed {
                file.speed = speed;
                return self.player.set_speed(speed);
            }
        }
        Ok(())
    }

    pub fn get_speed(&self) -> Result<f64> {
        if let Some(file) = &self.status.file {
            return Ok(file.speed);
        }
        self.player.get_speed()
    }

    pub fn get_position(&self) -> Result<Duration> {
        self.player.get_position()
    }

    pub fn load(&mut self, play: PlayingFile) -> Result<()> {
        if let Some(file) = &self.status.file {
            if file.video.as_str().eq(play.video.as_str()) {
                self.seek(play)
            } else {
                self.open(play)
            }
        } else {
            self.open(play)
        }
    }

    pub fn unload(&mut self) {
        self.status.file = None;
        self.status.file_loaded = false;
    }

    fn seek(&mut self, play: PlayingFile) -> Result<()> {
        if let Some(file) = self.status.file.clone() {
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

    fn open(&mut self, play: PlayingFile) -> Result<()> {
        self.status.file = Some(play.clone());
        if let Some(path) = play.video.to_path_str(&self.db) {
            self.status.file_loaded = true;
            self.player.open(path, play.paused, play.pos)
        } else {
            self.status.file_loaded = false;
            Ok(())
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        if !self.status.file_loaded {
            if let Some(file) = self.status.file.clone() {
                if let Some(path) = file.video.to_path_str(&self.db) {
                    self.status.file_loaded = true;
                    return self.player.open(path, file.paused, file.pos);
                }
            }
        }
        Ok(())
    }

    pub fn playing_file(&self) -> Option<PlayingFile> {
        if let Some(file) = self.status.file.clone() {
            return Some(file);
        }
        None
    }

    pub fn is_seeking(&self) -> Result<bool> {
        self.player.is_seeking()
    }

    pub async fn recv(&mut self) -> Result<MediaPlayerEvent> {
        self.player.receive_event().await
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
