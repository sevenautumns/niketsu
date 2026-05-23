pub mod communicator;
pub mod player;
pub mod ui;

pub use communicator::{ApiCommunicator, CommHandle};
pub use player::NoopPlayer;
pub use ui::{ApiUi, UiHandle, UiNotification};

use niketsu_core::builder::CoreBuilder;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::player::MediaPlayerTrait;
use niketsu_core::video_provider::VideoProvider;
use niketsu_video_server::VideoServer;
use tokio::task::JoinHandle;

pub struct CoreSession {
    pub ui: UiHandle,
    pub comm: CommHandle,
    task: JoinHandle<()>,
}

impl CoreSession {
    pub fn start(config: Config) -> Self {
        Self::start_with(config, Box::new(NoopPlayer))
    }

    pub fn start_with(config: Config, player: Box<dyn MediaPlayerTrait>) -> Self {
        let (api_ui, ui) = ui::api_ui();
        let (api_comm, comm) = communicator::api_communicator();

        let core = CoreBuilder::builder()
            .ui(Box::new(api_ui))
            .player(player)
            .communicator(Box::new(api_comm))
            .file_database(Box::new(FileDatabase::default()))
            .video_server(Box::new(VideoServer::default()))
            .video_provider(Box::new(VideoProvider::default()))
            .config(config)
            .build();

        let task = tokio::spawn(async move { core.run().await });

        Self { ui, comm, task }
    }

    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

#[cfg(test)]
mod tests {
    use niketsu_core::config::Config;

    use super::*;

    #[tokio::test]
    async fn core_session_starts_and_stops() {
        let session = CoreSession::start(Config::default());
        session.stop().await;
    }
}
