use std::time::Duration;

use tokio::time::Interval;
use tracing::trace;

use super::communicator::VideoStatusMsg;
use super::player::MediaPlayerTrait;
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
        let video = model.player.playing_video();
        let position = model.player.get_position();
        // the speed the room agreed on, which is not necessarily what the
        // local player was last told (a user turning the knob in mpv reaches
        // us as an event)
        let speed = model.player.host_speed();
        let paused = model.player.is_paused().unwrap_or(true);
        let cache = model.player.cache_available();
        let file_loaded = model.player.video_loaded();
        model.sync.observe_local_position(position);
        // only the host's clock is the room's reference
        let Some(host) = &mut model.host else {
            return;
        };
        host.select.position = position.unwrap_or_default();
        model.communicator.send(
            VideoStatusMsg {
                video,
                position,
                speed,
                paused,
                file_loaded,
                cache,
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;
    use tokio::time::timeout;

    use super::*;
    use crate::builder::CoreBuilder;
    use crate::communicator::{MockCommunicatorTrait, OutgoingMessage, PlaylistMsg, SelectMsg};
    use crate::config::Config;
    use crate::file_database::MockFileDatabaseTrait;
    use crate::player::MockMediaPlayerTrait;
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
    fn test_playlist_change() {
        let mut communicator = MockCommunicatorTrait::default();
        let mut player = MockMediaPlayerTrait::default();
        let ui = MockUserInterfaceTrait::default();
        let file_database = MockFileDatabaseTrait::default();
        let video_server = MockVideoServerTrait::default();
        let video_provider = MockVideoProviderTrait::default();

        let video = Video::from("video2");
        let position = Some(Duration::from_secs(15));
        let speed = 1.5;
        let paused = true;
        let config = Config::default();
        let cache = true;
        let message = OutgoingMessage::from(VideoStatusMsg {
            video: Some(video.clone()),
            position,
            speed,
            paused,
            file_loaded: true,
            cache,
        });

        player.expect_playing_video().return_const::<Video>(video);
        player.expect_get_position().return_const(position);
        player.expect_set_speed().return_const(());
        player.expect_is_paused().return_const(paused);
        player.expect_cache_available().return_const(cache);
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
            .file_database(Box::new(file_database))
            .video_server(Box::new(video_server))
            .video_provider(Box::new(video_provider))
            .config(config)
            .build();

        // the room's speed is what the status reports
        core.model.player.set_speed(speed);
        core.model.host = Some(host_room());

        Heartbeat.handle(&mut core.model);

        let select = &core.model.host.as_ref().unwrap().select;
        assert_eq!(
            select.position,
            Duration::from_secs(15),
            "late joiners must start where the host's clock is"
        );
    }

    fn host_room() -> HostRoom {
        HostRoom::new(
            "room".into(),
            PlaylistMsg::default(),
            SelectMsg {
                actor: "host".into(),
                position: Duration::ZERO,
                video: None,
            },
        )
    }

    #[test]
    fn clients_do_not_send_heartbeats() {
        let communicator = MockCommunicatorTrait::default();
        let mut player = MockMediaPlayerTrait::default();

        player.expect_playing_video().return_const(None::<Video>);
        player
            .expect_get_position()
            .return_const(Some(Duration::from_secs(3)));
        player.expect_is_paused().return_const(true);
        player.expect_cache_available().return_const(false);
        player.expect_video_loaded().return_const(false);

        let mut core = CoreBuilder::builder()
            .communicator(Box::new(communicator))
            .player(Box::new(player))
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
