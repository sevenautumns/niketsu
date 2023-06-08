use std::ops::Deref;
use std::sync::Arc;

use arcstr::ArcStr;
use url::Url;

use super::player::PlayerVideo;
use crate::file_database::FileStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistVideo {
    inner: Arc<PlaylistVideoInner>,
}

impl Deref for PlaylistVideo {
    type Target = PlaylistVideoInner;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl From<PlaylistVideoInner> for PlaylistVideo {
    fn from(value: PlaylistVideoInner) -> Self {
        let inner = Arc::new(value);
        Self { inner }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistVideoInner {
    File(ArcStr),
    Url(Url),
}

impl PlaylistVideoInner {
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    pub fn to_path_str(&self, db: &FileStore) -> Option<String> {
        match self {
            PlaylistVideoInner::File(name) => match db.find_file(name) {
                Some(entry) => Some(entry.path().as_os_str().to_str()?.to_string()),
                _ => None,
            },
            PlaylistVideoInner::Url(url) => Some(url.as_str().to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            PlaylistVideoInner::File(name) => name,
            PlaylistVideoInner::Url(url) => url.as_str(),
        }
    }

    pub fn to_player_video(&self, db: &FileStore) -> Option<PlayerVideo> {
        match self {
            PlaylistVideoInner::Url(url) => Some(PlayerVideo::Url(url.clone())),
            PlaylistVideoInner::File(name) => db.find_file(name).map(PlayerVideo::File),
        }
    }
}

impl AsRef<str> for PlaylistVideo {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for PlaylistVideo {
    fn from(value: &str) -> Self {
        PlaylistVideoInner::from(value).into()
    }
}

impl From<&str> for PlaylistVideoInner {
    fn from(value: &str) -> Self {
        if let Ok(url) = Url::parse(value) {
            Self::Url(url)
        } else {
            Self::File(value.into())
        }
    }
}

impl From<&ArcStr> for PlaylistVideo {
    fn from(value: &ArcStr) -> Self {
        PlaylistVideoInner::from(value).into()
    }
}

impl From<&ArcStr> for PlaylistVideoInner {
    fn from(value: &ArcStr) -> Self {
        if let Ok(url) = Url::parse(value) {
            Self::Url(url)
        } else {
            Self::File(value.clone())
        }
    }
}
