use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use niketsu::cli::Args;
use niketsu_communicator::P2PCommunicator;
use niketsu_core::builder::CoreBuilder;
use niketsu_core::config::Config;
use niketsu_core::file_database::FileDatabase;
use niketsu_core::logging::setup_logger;
use niketsu_core::ui::UserInterfaceTrait;
use niketsu_core::video_provider::VideoProvider;
use niketsu_mpv::Mpv;
use niketsu_video_server::VideoServer;

#[cfg(all(not(feature = "ratatui"), not(feature = "iced")))]
compile_error!(r#"any ui feature is required ["ratatui", "iced"]"#);

async fn run_app(args: Args) -> Result<()> {
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
    let communicator = P2PCommunicator::default();
    let video_server = VideoServer::default();
    let video_provider = VideoProvider::default();
    let mut file_database = FileDatabase::default();
    if !args.skip_database_refresh {
        file_database = FileDatabase::new(config.media_dirs.iter().map(PathBuf::from).collect());
    }

    let core = CoreBuilder::builder()
        .ui(view)
        .player(Box::new(player))
        .communicator(Box::new(communicator))
        .file_database(Box::new(file_database))
        .video_server(Box::new(video_server))
        .video_provider(Box::new(video_provider))
        .chat_logger(chat_logger)
        .config(config)
        .build();

    tokio::task::spawn(async move { core.run().await });
    ui_fn.await
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Iced uses winit which owns the main thread and drives the Cocoa loop itself.
    #[cfg(feature = "iced")]
    if matches!(args.ui, niketsu::cli::UI::Iced) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(run_app(args));
    }

    // Ratatui is a terminal UI and never touches Cocoa. On macOS, mpv needs NSApplication
    // running on the main thread to display its video window. We run the app logic on a
    // background thread and occupy the main thread with the Cocoa event loop.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    std::thread::spawn(move || {
        if let Err(e) = rt.block_on(run_app(args)) {
            eprintln!("Error: {e:?}");
            std::process::exit(1);
        }
        std::process::exit(0);
    });

    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;

        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            // Blocks the main thread; mpv uses this loop to drive its video window.
            unsafe { app.run() };
        }
    }

    #[cfg(not(target_os = "macos"))]
    loop {
        std::thread::park();
    }

    Ok(())
}
