use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use niketsu::cli::Args;
use niketsu_communicator::WebsocketCommunicator;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::logging::setup_logger;
use niketsu_core::ui::UserInterfaceTrait;
use niketsu_mpv::Mpv;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let chat_logger = setup_logger(args.log_level_terminal.into(), args.log_level_chat.into())?;

    let mut config: Config = Config::load_or_default();

    if let Some(auto_connect) = args.auto_connect {
        config.auto_connect = auto_connect
    }

    let view: Box<dyn UserInterfaceTrait>;
    let ui_fn;
    match args.ui {
        #[cfg(feature = "iced")]
        niketsu::cli::UI::Iced => {
            let iced = niketsu_iced::IcedUI::create(config.clone());
            view = Box::new(iced.0);
            ui_fn = iced.1;
        }
        #[cfg(feature = "ratatui")]
        niketsu::cli::UI::Ratatui => {
            let ratatui = niketsu_ratatui::RatatuiUI::create(config.clone());
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
        .ui(view)
        .player(Box::new(player))
        .communicator(Box::new(communicator))
        .file_database(Box::new(file_database))
        .chat_logger(chat_logger)
        .config(config)
        .build();

    tokio::task::spawn(async move { core.run().await });

    ui_fn.await
}
