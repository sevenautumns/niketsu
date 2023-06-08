use core_mock::CoreMock;
use niketsu::tauri_ui::TauriUI;

#[path = "../core_mock.rs"]
mod core_mock;

#[tokio::main]
async fn main() {
    let tauri = TauriUI::new();
    let core = CoreMock::new(tauri);
    core.run().await;
}
