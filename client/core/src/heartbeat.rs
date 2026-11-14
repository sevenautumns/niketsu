use std::time::Duration;

use tokio::time::Interval;
use tracing::trace;

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
        trace!("heartbeat");
        // only the host's clock is the room's reference
        model.publish_beat();
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::timeout;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::{HostInstruction, MockCommunicatorTrait, OutgoingMessage};
    use crate::config::Config;
    use crate::file_database::MockFileDatabaseTrait;
    use crate::player::{MediaPlayerTrait, MockMediaPlayerTrait};
    use crate::playlist::Video;
    use crate::room::HostRoom;
    use crate::ui::MockUserInterfaceTrait;
    use crate::{MockVideoProviderTrait, MockVideoServerTrait};

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
    fn the_hosts_heartbeat_refreshes_the_rooms_clock() {
        let mut communicator = MockCommunicatorTrait::default();
        let mut player = MockMediaPlayerTrait::default();

        let video = Video::from("video2");
        player
            .expect_playing_video()
            .return_const::<Video>(video.clone());
        player
            .expect_get_position()
            .return_const(Some(Duration::from_secs(15)));
        player.expect_set_speed().return_const(());
        player.expect_is_paused().return_const(true);
        player.expect_video_loaded().return_const(true);
        communicator
            .expect_send()
            .withf(move |msg| {
                let OutgoingMessage::Host(HostInstruction::RoomState(state)) = msg else {
                    return false;
                };
                state.order() == (0, 1)
                    && state.video.as_ref() == Some(&video)
                    && state.position == Duration::from_secs(15)
                    && state.speed == 1.5
                    && state.paused
                    && state.file_loaded
            })
            .once()
            .return_const(());

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
            .ui(Box::new(MockUserInterfaceTrait::default()))
            .file_database(Box::new(MockFileDatabaseTrait::default()))
            .video_server(Box::new(MockVideoServerTrait::default()))
            .video_provider(Box::new(MockVideoProviderTrait::default()))
            .config(Config::default())
            .build();

        // the room's speed is what the state reports
        core.model.player.set_speed(1.5);
        core.model.host = Some(HostRoom::new("room".into()));

        Heartbeat.handle(&mut core.model);
    }

    #[test]
    fn clients_do_not_send_heartbeats() {
        let mut core = CoreBuilder::builder()
            .communicator(Box::new(MockCommunicatorTrait::default()))
            .player(Box::new(MockMediaPlayerTrait::default()))
            .ui(Box::new(MockUserInterfaceTrait::default()))
            .file_database(Box::new(MockFileDatabaseTrait::default()))
            .video_server(Box::new(MockVideoServerTrait::default()))
            .video_provider(Box::new(MockVideoProviderTrait::default()))
            .config(Config::default())
            .build();

        // the mock communicator fails the test on any send
        Heartbeat.handle(&mut core.model);
    }
}
