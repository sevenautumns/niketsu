use anyhow::Result;
use niketsu_communicator::WebsocketCommunicator;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_iced::config::Config as IcedConfig;
use niketsu_iced::IcedUI;
use niketsu_mpv::Mpv;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let config: Config = Config::load_or_default();
    let iced_config: IcedConfig = IcedConfig::load_or_default();

    let (view, ui_fn) = IcedUI::new(iced_config, config.clone());
    let player = Mpv::new().unwrap();
    let communicator = WebsocketCommunicator::default();

    let core = CoreBuilder::builder()
        .username(config.username)
        .password(config.password)
        .room(config.room)
        .ui(Box::new(view))
        .player(Box::new(player))
        .communicator(Box::new(communicator))
        .playlist(Box::<PlaylistHandler>::default())
        .file_database(Box::<FileDatabase>::default())
        .build();

    tokio::task::spawn(async move { core.run().await });

    ui_fn()
}
