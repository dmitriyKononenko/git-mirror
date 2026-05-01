// main.rs — Binary entry point (thin wrapper around the library crate)
//
// All logic lives in lib.rs and its modules.
// main.rs only starts the window — nothing testable lives here.

use eframe::egui;
use git_mirror::app::CopyApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Copy Automation")
            .with_inner_size([660.0, 540.0])
            .with_min_inner_size([500.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "copy-automation",
        options,
        Box::new(|cc| Ok(Box::new(CopyApp::new(cc)))),
    )
}
