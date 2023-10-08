use std::sync::Arc;

use arc_swap::ArcSwap;
use iced::Executor;
use niketsu_core::util::RingBuffer;
use once_cell::sync::Lazy;

pub mod config;
mod main_window;
mod message;
mod settings_window;
mod styling;
mod view;
mod widget;

pub static TEXT_SIZE: Lazy<ArcSwap<f32>> = Lazy::new(|| ArcSwap::new(Arc::new(14.0)));

pub use self::view::View as IcedUI;

#[derive(Debug)]
pub struct PreExistingTokioRuntime;

impl Executor for PreExistingTokioRuntime {
    fn new() -> Result<Self, futures::io::Error>
    where
        Self: Sized,
    {
        Ok(Self)
    }

    #[allow(clippy::let_underscore_future)]
    fn spawn(&self, future: impl futures::Future<Output = ()> + iced_futures::MaybeSend + 'static) {
        let _ = tokio::task::spawn(future);
    }
}
