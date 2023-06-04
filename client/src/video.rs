use std::sync::Arc;
use std::time::Duration;

use url::Url;

use crate::client::database::FileDatabaseSender;

#[derive(Debug, Clone, Eq)]
pub enum Video {
    File { name: String },
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
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    pub fn from_string(video: String) -> Self {
        if let Ok(url) = Url::parse(&video) {
            Self::Url(url)
        } else {
            Self::File { name: video }
        }
    }

    pub fn to_path_str(&self, db: &Arc<FileDatabaseSender>) -> Option<String> {
        match self {
            Video::File { name } => match db.find_file(name) {
                Ok(Some(file)) => Some(file.path.as_os_str().to_str()?.to_string()),
                _ => None,
            },
            Video::Url(url) => Some(url.as_str().to_string()),
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
    pub paused: bool,
    pub speed: f64,
    pub pos: Duration,
}
