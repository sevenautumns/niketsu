use anyhow::Result;
use niketsu::communicator::WebsocketCommunicator;
use niketsu::config::Config;
use niketsu::core::build::CoreBuilder;
use niketsu::file_database::FileDatabase;
use niketsu::iced_ui::IcedUI;
use niketsu::player::mpv::Mpv;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let config: Config = Config::load_or_default();

    let (view, ui_fn) = IcedUI::new(config.clone());
    let database = FileDatabase::default();
    let player = Mpv::new().unwrap();
    let communicator = WebsocketCommunicator::default();

    let core = CoreBuilder::builder()
        .username(config.username)
        .password(config.password)
        .room(config.room)
        .ui(Box::new(view))
        .database(Box::new(database))
        .player(Box::new(player))
        .communicator(Box::new(communicator))
        .build();

    tokio::task::spawn(async move { core.run().await });

    ui_fn()
}
