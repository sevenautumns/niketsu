use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use niketsu::cli::Args;
use niketsu_communicator::WebsocketCommunicator;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::playlist::handler::PlaylistHandler;
use niketsu_core::ui::UserInterfaceTrait;
use niketsu_iced::config::Config as IcedConfig;
use niketsu_iced::IcedUI;
use niketsu_mpv::Mpv;
use niketsu_ratatui::RatatuiUI;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let args = Args::parse();
    let mut config: Config = Config::load_or_default();
    let iced_config: IcedConfig = IcedConfig::load_or_default();

    if let Some(auto_login) = args.auto_login {
        config.auto_login = auto_login
    }

    let view: Box<dyn UserInterfaceTrait>;
    let ui_fn;
    match args.ui {
        niketsu::cli::UI::Iced => {
            let iced = IcedUI::create(iced_config, config.clone());
            view = Box::new(iced.0);
            ui_fn = iced.1;
        }
        niketsu::cli::UI::Ratatui => {
            let ratatui = RatatuiUI::create(config.clone());
            view = Box::new(ratatui.0);
            ui_fn = ratatui.1;
        }
    }
    let player = Mpv::new().unwrap();
    let communicator = WebsocketCommunicator::default();
    let mut file_database = FileDatabase::default();
    if !args.skip_database_refresh {
        file_database = FileDatabase::new(config.media_dirs.iter().map(PathBuf::from).collect());
    }

    let core = CoreBuilder::builder()
        .username(config.username)
        .password(config.password)
        .room(config.room)
        .ui(view)
        .player(Box::new(player))
        .communicator(Box::new(communicator))
        .file_database(Box::new(file_database))
        .playlist(Box::<PlaylistHandler>::default())
        .build();

    tokio::task::spawn(async move { core.run().await });

    ui_fn()
}
