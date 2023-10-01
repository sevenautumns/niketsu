use fake::Fake;
use typed_builder::TypedBuilder;

use crate::communicator::CommunicatorTrait;
use crate::file_database::FileDatabaseTrait;
use crate::player::MediaPlayerTrait;
use crate::playlist::PlaylistHandlerTrait;
use crate::ui::UserInterfaceTrait;
use crate::user::UserStatus;
use crate::{Core, CoreModel};

#[derive(TypedBuilder)]
#[builder(build_method(into = Core))]
pub struct CoreBuilder {
    ui: Box<dyn UserInterfaceTrait>,
    player: Box<dyn MediaPlayerTrait>,
    communicator: Box<dyn CommunicatorTrait>,
    playlist: Box<dyn PlaylistHandlerTrait>,
    file_database: Box<dyn FileDatabaseTrait>,
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
            database: builder.file_database,
            player: builder.player,
            ui: builder.ui,
            user: UserStatus {
                name: builder.username.unwrap_or_else(whoami::username),
                ready: false,
            },
            room: builder.room.unwrap_or_else(generate_room_name),
            password: builder.password,
            playlist: builder.playlist,
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
