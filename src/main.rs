// main.rs — Binary entry point (thin wrapper around the library crate)
//
// All logic lives in lib.rs and its modules.
// main.rs only starts the window — nothing testable lives here.

use eframe::egui;
use git_mirror::app::GitMirrorApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Git Mirror")
            .with_inner_size([660.0, 540.0])
            .with_min_inner_size([500.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "git-mirror",
        options,
        Box::new(|cc| Ok(Box::new(GitMirrorApp::new(cc)))),
    )
}
