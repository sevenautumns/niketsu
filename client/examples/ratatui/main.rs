use anyhow::Result;
use core_mock::CoreMock;
use niketsu_core::config::Config;
use niketsu_ratatui::RatatuiUI;

#[path = "../core_mock.rs"]
mod core_mock;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    let (ratatui, run) = RatatuiUI::create(config);
    let core = CoreMock::new(ratatui);
    tokio::task::spawn(core.run());
    run()
}
