use std::path::PathBuf;
use std::time::{Duration, Instant};

use url::Url;

#[derive(Debug, Clone, Eq)]
pub enum Video {
    File { name: String, path: Option<PathBuf> },
    Url(Url),
}

impl PartialEq for Video {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Video::File { name: n1, .. }, Video::File { name: n2, .. }) => n1.eq(n2),
            (Video::Url(u1), Video::Url(u2)) => u1.eq(u2),
            _ => false,
        }
    }
}

impl Video {
    pub fn from_string(video: String) -> Self {
        if let Ok(url) = Url::parse(&video) {
            Self::Url(url)
        } else {
            Self::File {
                name: video,
                path: None,
            }
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Video::File { name, .. } => name,
            Video::Url(url) => url.as_str(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayingFile {
    pub video: Video,
    pub last_seek: Option<SeekEvent>,
    pub heartbeat: bool,
}

// impl PlayingFile {
//     pub fn subscribe(&self) -> Subscription<MainMessage> {
//         if self.heartbeat {
//             iced::subscription::channel(
//                 std::any::TypeId::of::<Self>(),
//                 1,
//                 |mut output| async move {
//                     loop {
//                         tokio::time::sleep(Duration::from_secs(5)).await;
//                         if let Err(e) = output.try_send(MainMessage::Heartbeat) {
//                             error!("{e:?}");
//                         }
//                     }
//                 },
//             )
//         } else {
//             Subscription::none()
//         }
//     }
// }

#[derive(Debug, Clone)]
pub struct SeekEvent {
    pub when: Instant,
    pub pos: Duration,
}

impl SeekEvent {
    pub fn pos(&self) -> Duration {
        self.pos
    }

    pub fn new(pos: Duration) -> Self {
        Self {
            when: Instant::now(),
            pos,
        }
    }
}

pub trait SeekEventExt {
    fn pos(&self) -> Option<Duration>;
}

impl SeekEventExt for Option<SeekEvent> {
    fn pos(&self) -> Option<Duration> {
        self.as_ref().map(|s| s.pos())
    }
}

// pub trait PlayingFileExt {
//     fn paused(&self) -> bool;
// }

// impl PlayingFileExt for Option<PlayingFile> {
//     fn paused(&self) -> bool {
//         match self {
//             Some(file) => file.paused,
//             None => true,
//         }
//     }
// }
