use fake::Fake;
use niketsu_core::communicator::CommunicatorTrait;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::player::MediaPlayerTrait;
use niketsu_core::playlist::PlaylistHandler;
use niketsu_core::ui::UserInterfaceTrait;
use niketsu_core::user::UserStatus;
use niketsu_core::{Core, CoreModel};
use typed_builder::TypedBuilder;

#[derive(TypedBuilder)]
#[builder(build_method(into = Core))]
pub struct CoreBuilder {
    ui: Box<dyn UserInterfaceTrait>,
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
            database: FileDatabase::default(),
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
