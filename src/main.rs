// use std::path::Path;
// use std::thread::sleep;
// use std::time::{Duration, Instant};

use anyhow::Result;
use iced::{Application, Settings};
// use log::debug;
// use tokio::runtime::Runtime;
use window::MainWindow;

// use crate::fs::FileDatabase;

pub mod file_table;
pub mod fs;
pub mod mpv;
pub mod window;
pub mod ws;

fn main() -> Result<()> {
    pretty_env_logger::init();

    // let rt = Runtime::new()?;
    // rt.spawn(async move {
    //     let now = Instant::now();
    //     let mut db = FileDatabase::new();
    //     // db.add_search_path(Path::new("/net/index/torrent_storage/anime").to_path_buf())
    //     db.add_search_path(Path::new("/nix/store").to_path_buf())
    //         // db.add_search_path(Path::new("/run/user/125030/gvfs/sftp:host=192.168.178.2,user=autumnal/media/torrent_storage/anime").to_path_buf())
    //         .await;
    //     // db.add_search_path(Path::new("/home/autumnal").to_path_buf());
    //     db.update().await.unwrap();
    //     debug!(
    //         "Found files: {} in {:?}",
    //         db.database().read().await.len(),
    //         now.elapsed()
    //     );

    //     // while let Some(a) = events.next().await {
    //     //     debug!("{a:?}");
    //     // }
    // });

    MainWindow::run(Settings::default())?;
    Ok(())
}
