use typed_builder::TypedBuilder;

use crate::communicator::CommunicatorTrait;
use crate::config::Config;
use crate::file_database::FileDatabaseTrait;
use crate::logging::ChatLogger;
use crate::player::wrapper::MediaPlayerWrapper;
use crate::player::MediaPlayerTrait;
use crate::playlist::handler::PlaylistHandler;
use crate::ui::UserInterfaceTrait;
use crate::video_provider::VideoProviderTrait;
use crate::{Core, CoreModel, VideoServerTrait};

#[derive(TypedBuilder)]
#[builder(build_method(into = Core))]
pub struct CoreBuilder {
    ui: Box<dyn UserInterfaceTrait>,
    player: Box<dyn MediaPlayerTrait>,
    communicator: Box<dyn CommunicatorTrait>,
    file_database: Box<dyn FileDatabaseTrait>,
    video_server: Box<dyn VideoServerTrait>,
    video_provider: Box<dyn VideoProviderTrait>,
    #[builder(default)]
    chat_logger: Option<ChatLogger>,
    config: Config,
}

impl From<CoreBuilder> for CoreModel {
    fn from(builder: CoreBuilder) -> Self {
        Self {
            communicator: builder.communicator,
            database: builder.file_database,
            player: MediaPlayerWrapper::new(builder.player),
            ui: builder.ui,
            config: builder.config,
            playlist: PlaylistHandler::default(),
            chat_logger: builder.chat_logger,
            video_server: builder.video_server,
            video_provider: builder.video_provider,
            ready: false,
            running: true,
        }
    }
}

impl From<CoreBuilder> for Core {
    fn from(builder: CoreBuilder) -> Self {
        Self {
            model: builder.into(),
        }
    }
}
