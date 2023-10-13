use std::time::Duration;

use tokio::time::Interval;

use super::communicator::NiketsuVideoStatus;
use super::{CoreModel, EventHandler};

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

pub struct Pacemaker {
    interval: Interval,
}

impl Default for Pacemaker {
    fn default() -> Self {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self { interval }
    }
}

impl Pacemaker {
    pub async fn recv(&mut self) -> Heartbeat {
        self.interval.tick().await;
        Heartbeat
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Heartbeat;

impl EventHandler for Heartbeat {
    fn handle(self, model: &mut CoreModel) {
        let filename = model.player.playing_video().map(|v| v.as_str().to_string());
        let position = model.player.get_position();
        let speed = model.player.get_speed();
        let paused = model.player.is_paused().unwrap_or(true);
        let file_loaded = model.player.video_loaded();
        model.communicator.send(
            NiketsuVideoStatus {
                filename,
                position,
                speed,
                paused,
                file_loaded,
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use arcstr::ArcStr;
    use mockall::predicate::eq;
    use tokio::time::timeout;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::{MockCommunicatorTrait, OutgoingMessage};
    use crate::file_database::MockFileDatabaseTrait;
    use crate::player::MockMediaPlayerTrait;
    use crate::playlist::{MockPlaylistHandlerTrait, Video, VideoInner};
    use crate::ui::MockUserInterfaceTrait;

    #[tokio::test]
    async fn test_pacemaker() {
        let mut pacemaker = Pacemaker::default();

        timeout(HEARTBEAT_INTERVAL, pacemaker.recv()).await.unwrap();
        timeout(HEARTBEAT_INTERVAL, pacemaker.recv()).await.unwrap();
        timeout(
            HEARTBEAT_INTERVAL.saturating_sub(Duration::from_millis(100)),
            pacemaker.recv(),
        )
        .await
        .unwrap_err();
    }

    #[test]
    fn test_playlist_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let mut player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let playlist_handler = MockPlaylistHandlerTrait::default();

        let video = ArcStr::from("video2");
        let position = Some(Duration::from_secs(15));
        let speed = 1.5;
        let paused = true;
        let message = OutgoingMessage::from(NiketsuVideoStatus {
            filename: Some(video.to_string()),
            position,
            speed,
            paused,
            file_loaded: true,
        });

        player
            .expect_playing_video()
            .return_const::<Video>(VideoInner::File(video).into());
        player.expect_get_position().return_const(position);
        player.expect_get_speed().return_const(speed);
        player.expect_is_paused().return_const(paused);
        player.expect_video_loaded().return_const(true);
        communicator
            .expect_send()
            .with(eq(message))
            .once()
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .playlist(Box::new(playlist_handler))
            .file_database(Box::new(file_database))
            .build();

        Heartbeat.handle(&mut core.model)
    }
}
