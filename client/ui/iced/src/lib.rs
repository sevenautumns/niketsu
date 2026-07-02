use iced::Executor;
use niketsu_core::util::RingBuffer;

mod config;
mod main_window;
mod message;
mod styling;
mod view;
mod widget;

pub const TEXT_SIZE: f32 = 14.0;

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
    fn spawn(&self, future: impl futures::Future<Output = ()> + Send + 'static) {
        let _ = tokio::task::spawn(future);
    }

    fn block_on<T>(&self, future: impl futures::Future<Output = T>) -> T {
        futures::executor::block_on(future)
    }
}
