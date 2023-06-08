use core_mock::CoreMock;
use niketsu::ratatui_ui::RatatuiUI;

#[path = "../core_mock.rs"]
mod core_mock;

#[tokio::main]
async fn main() {
    let ratatui = RatatuiUI::new();
    let core = CoreMock::new(ratatui);
    core.run().await;
}
