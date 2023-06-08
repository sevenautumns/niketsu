use fake::Fake;
use typed_builder::TypedBuilder;

use super::communicator::CommunicatorTrait;
use super::file_database::FileDatabaseTrait;
use super::player::MediaPlayerTrait;
use super::ui::UserInterfaceTrait;
use super::user::UserStatus;
use super::{Core, CoreModel};
use crate::playlist::PlaylistHandler;

#[derive(TypedBuilder)]
#[builder(build_method(into = Core))]
pub struct CoreBuilder {
    ui: Box<dyn UserInterfaceTrait>,
    database: Box<dyn FileDatabaseTrait>,
    player: Box<dyn MediaPlayerTrait>,
    communicator: Box<dyn CommunicatorTrait>,
    #[builder(default, setter(strip_option))]
    username: Option<String>,
    #[builder(default, setter(strip_option))]
    room: Option<String>,
    #[builder(default, setter(strip_option))]
    password: Option<String>,
}

fn generate_room_name() -> String {
    fake::faker::lorem::en::Word().fake()
}

impl From<CoreBuilder> for CoreModel {
    fn from(builder: CoreBuilder) -> Self {
        Self {
            communicator: builder.communicator,
            database: builder.database,
            player: builder.player,
            ui: builder.ui,
            user: UserStatus {
                name: builder.username.unwrap_or_else(whoami::username),
                ready: false,
            },
            room: builder.room.unwrap_or_else(generate_room_name),
            password: builder.password,
            playlist: PlaylistHandler::default(),
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
