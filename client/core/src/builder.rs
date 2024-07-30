use typed_builder::TypedBuilder;

use crate::communicator::CommunicatorTrait;
use crate::config::Config;
use crate::file_database::FileDatabaseTrait;
use crate::logging::ChatLogger;
use crate::player::wrapper::MediaPlayerWrapper;
use crate::player::MediaPlayerTrait;
use crate::playlist::PlaylistHandlerTrait;
use crate::ui::UserInterfaceTrait;
use crate::{Core, CoreModel};

#[derive(TypedBuilder)]
#[builder(build_method(into = Core))]
pub struct CoreBuilder {
    ui: Box<dyn UserInterfaceTrait>,
    player: Box<dyn MediaPlayerTrait>,
    communicator: Box<dyn CommunicatorTrait>,
    playlist: Box<dyn PlaylistHandlerTrait>,
    file_database: Box<dyn FileDatabaseTrait>,
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
            playlist: builder.playlist,
            chat_logger: builder.chat_logger,
            ready: false,
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
