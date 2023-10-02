use core_mock::CoreMock;
use niketsu_ratatui::RatatuiUI;

#[path = "../core_mock.rs"]
mod core_mock;

#[tokio::main]
async fn main() {
    let ratatui = RatatuiUI::default();
    let core = CoreMock::new(ratatui);
    core.run().await;
}
