#![warn(clippy::unwrap_used)]
#![warn(clippy::too_many_lines)]

use std::sync::Arc;

use arc_swap::ArcSwap;
use log::*;
use once_cell::sync::Lazy;

pub mod cli;
pub mod communicator;
pub mod config;
pub mod core;
pub mod file_database;
pub mod iced_ui;
pub mod player;
pub mod playlist;
pub mod ratatui_ui;
pub mod rooms;
pub mod styling;
pub mod tauri_ui;
pub mod util;

pub static TEXT_SIZE: Lazy<ArcSwap<f32>> = Lazy::new(|| ArcSwap::new(Arc::new(14.0)));

#[macro_export]
macro_rules! log {
    ($result:expr) => {
        if let Err(err) = $result {
            log::error!("{:?}", err);
        }
    };
    ($result:expr, $default:expr) => {
        match $result {
            Ok(ok_val) => ok_val,
            Err(err) => {
                log::error!("{:?}", err);
                $default
            }
        }
    };
}
