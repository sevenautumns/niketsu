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
        model.heartbeat();
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::timeout;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::MockCommunicatorTrait;
    use crate::config::Config;
    use crate::file_database::MockFileDatabaseTrait;
    use crate::player::MockMediaPlayerTrait;
    use crate::room::Role;
    use crate::sync::ClientSync;
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

    /// The heartbeat is the authority's: a follower's does nothing, and the
    /// mocks fail the test on any call.
    #[test]
    fn a_followers_heartbeat_does_nothing() {
        let mut core = CoreBuilder::builder()
            .communicator(Box::new(MockCommunicatorTrait::with_peer()))
            .player(Box::new(MockMediaPlayerTrait::default()))
            .ui(Box::new(MockUserInterfaceTrait::default()))
            .file_database(Box::new(MockFileDatabaseTrait::default()))
            .video_server(Box::new(MockVideoServerTrait::default()))
            .video_provider(Box::new(MockVideoProviderTrait::default()))
            .config(Config::default())
            .build();
        core.model.role = Role::Follower(ClientSync::default());

        Heartbeat.handle(&mut core.model);
    }

    /// A core in no room follows its own state and has nobody to publish
    /// it to: the mock communicator fails the test on any send.
    #[test]
    fn a_core_in_no_room_publishes_nothing() {
        let mut player = MockMediaPlayerTrait::default();
        player.expect_is_paused().return_const(None);
        let mut ui = MockUserInterfaceTrait::default();
        ui.expect_user_list().once().return_const(());
        let mut core = CoreBuilder::builder()
            .communicator(Box::new(MockCommunicatorTrait::with_peer()))
            .player(Box::new(player))
            .ui(Box::new(ui))
            .file_database(Box::new(MockFileDatabaseTrait::default()))
            .video_server(Box::new(MockVideoServerTrait::default()))
            .video_provider(Box::new(MockVideoProviderTrait::default()))
            .config(Config::default())
            .build();

        Heartbeat.handle(&mut core.model);
        Heartbeat.handle(&mut core.model);
    }
}
