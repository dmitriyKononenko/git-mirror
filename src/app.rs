// app.rs — egui application state and UI rendering

use crate::config::{Config, Destination};
use crate::git;
use crate::git::ChangedFile;
use crate::sync;
use eframe::egui;
use rfd::FileDialog;

const MAX_FILES_SHOWN: usize = 10;

pub struct GitMirrorApp {
    config: Config,
    new_dest_label: String,
    status_message: Option<String>,
    changed_files: Vec<ChangedFile>,
    show_all_files: bool,
}

impl GitMirrorApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load().unwrap_or_default();
        let mut app = Self {
            config,
            new_dest_label: String::new(),
            status_message: None,
            changed_files: Vec::new(),
            show_all_files: false,
        };
        app.refresh_changed_files();
        app
    }

    fn refresh_changed_files(&mut self) {
        self.changed_files.clear();
        self.show_all_files = false;

        let Some(ref repo_path) = self.config.source_repo else {
            return;
        };

        let repo = match git::open_repo(repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = Some(format!("❌ {}", e));
                return;
            }
        };

        match git::get_worktree_changes(&repo) {
            Ok(files) => self.changed_files = files,
            Err(e) => self.status_message = Some(format!("❌ Git error: {}", e)),
        }
    }

    /// Shared helper that opens the repo and returns worktree changes + all tracked files.
    /// Avoids repeating the same open/error-handling in every sync method.
    ///
    /// Returns `(worktree_files, repo)` — the repo is returned so callers can also
    /// call `get_all_tracked_files` without re-opening it.
    fn open_repo_and_changes(&mut self) -> Option<(git2::Repository, Vec<ChangedFile>)> {
        let source = self.config.source_repo.clone()?;
        let repo = match git::open_repo(&source) {
            Ok(r) => r,
            Err(e) => {
                self.status_message = Some(format!("❌ {}", e));
                return None;
            }
        };
        let worktree = match git::get_worktree_changes(&repo) {
            Ok(f) => f,
            Err(e) => {
                self.status_message = Some(format!("❌ {}", e));
                return None;
            }
        };
        Some((repo, worktree))
    }

    // ── Sync all destinations ──────────────────────────────────────────────

    fn run_sync(&mut self) {
        if self.config.source_repo.is_none() {
            self.status_message = Some("❌ No source repository selected".into());
            return;
        }
        let source = self.config.source_repo.clone().unwrap();
        let Some((repo, worktree_files)) = self.open_repo_and_changes() else {
            return;
        };

        let mut total_copied = 0;
        let mut total_errors = 0;

        for dest in &self.config.destinations {
            let files = if sync::is_destination_empty(&dest.path) {
                match git::get_all_tracked_files(&repo) {
                    Ok(f) => f,
                    Err(e) => {
                        self.status_message = Some(format!("❌ {}", e));
                        return;
                    }
                }
            } else {
                worktree_files.clone()
            };
            let result = sync::sync_files(&source, dest, &files);
            total_copied += result.files_copied;
            total_errors += result.errors.len();
        }

        self.status_message = Some(status_msg(
            "Synced",
            total_copied,
            self.config.destinations.len(),
            total_errors,
        ));
        self.refresh_changed_files();
    }

    fn run_force_sync(&mut self) {
        if self.config.source_repo.is_none() {
            self.status_message = Some("❌ No source repository selected".into());
            return;
        }
        let source = self.config.source_repo.clone().unwrap();
        let Some((repo, _)) = self.open_repo_and_changes() else {
            return;
        };

        let all_files = match git::get_all_tracked_files(&repo) {
            Ok(f) => f,
            Err(e) => {
                self.status_message = Some(format!("❌ {}", e));
                return;
            }
        };

        let mut total_copied = 0;
        let mut total_errors = 0;

        for dest in &self.config.destinations {
            let result = sync::sync_files(&source, dest, &all_files);
            total_copied += result.files_copied;
            total_errors += result.errors.len();
        }

        self.status_message = Some(status_msg(
            "Force synced",
            total_copied,
            self.config.destinations.len(),
            total_errors,
        ));
        self.refresh_changed_files();
    }

    // ── Sync a single destination by index ────────────────────────────────
    //
    // WHY index instead of passing the Destination directly?
    // Rust's borrow checker won't let us hold a reference into `self.config.destinations`
    // while also calling `&mut self` methods. Using an index sidesteps this:
    // we look up the destination inside the method after we have `&mut self`.

    fn run_sync_one(&mut self, idx: usize) {
        let Some(source) = self.config.source_repo.clone() else {
            self.status_message = Some("❌ No source repository selected".into());
            return;
        };
        let Some((repo, worktree_files)) = self.open_repo_and_changes() else {
            return;
        };
        let Some(dest) = self.config.destinations.get(idx) else {
            return;
        };

        let files = if sync::is_destination_empty(&dest.path) {
            match git::get_all_tracked_files(&repo) {
                Ok(f) => f,
                Err(e) => {
                    self.status_message = Some(format!("❌ {}", e));
                    return;
                }
            }
        } else {
            worktree_files
        };

        let result = sync::sync_files(&source, dest, &files);
        let label = dest.label.clone();
        self.status_message = Some(status_msg(
            &format!("Synced [{label}]"),
            result.files_copied,
            1,
            result.errors.len(),
        ));
        self.refresh_changed_files();
    }

    fn run_force_sync_one(&mut self, idx: usize) {
        let Some(source) = self.config.source_repo.clone() else {
            self.status_message = Some("❌ No source repository selected".into());
            return;
        };
        let Some((repo, _)) = self.open_repo_and_changes() else {
            return;
        };
        let Some(dest) = self.config.destinations.get(idx) else {
            return;
        };

        let all_files = match git::get_all_tracked_files(&repo) {
            Ok(f) => f,
            Err(e) => {
                self.status_message = Some(format!("❌ {}", e));
                return;
            }
        };

        let result = sync::sync_files(&source, dest, &all_files);
        let label = dest.label.clone();
        self.status_message = Some(status_msg(
            &format!("Force synced [{label}]"),
            result.files_copied,
            1,
            result.errors.len(),
        ));
        self.refresh_changed_files();
    }
}

/// Builds a consistent status string — extracted to avoid repetition.
/// In TS you'd write a small helper function at module scope; same idea here.
fn status_msg(verb: &str, files: usize, dests: usize, errors: usize) -> String {
    format!(
        "✅ {} {} file(s) to {} destination(s){}",
        verb,
        files,
        dests,
        if errors > 0 {
            format!(" ({} errors)", errors)
        } else {
            String::new()
        }
    )
}

impl eframe::App for GitMirrorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Collect pending actions here to avoid borrow conflicts.
        // egui closures borrow `ui` mutably; we can't also call `&mut self` methods
        // inside them. So we record *what* to do, then execute it after the UI pass.
        //
        // This is a common egui pattern — think of it like React's setState batching.
        let mut action: Option<UiAction> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔀 Git Mirror");
            ui.separator();

            // ── Source Repository ──────────────────────────────────────────
            ui.label("Source Repository:");
            ui.horizontal(|ui| {
                let display = self
                    .config
                    .source_repo
                    .as_deref()
                    .unwrap_or("No folder selected…");
                ui.add_enabled(
                    false,
                    egui::TextEdit::singleline(&mut display.to_owned()).desired_width(320.0),
                );
                if ui.button("📂 Browse…").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        action = Some(UiAction::SetSource(path.to_string_lossy().into_owned()));
                    }
                }
                if self.config.source_repo.is_some() && ui.small_button("✕").clicked() {
                    action = Some(UiAction::ClearSource);
                }
            });

            ui.add_space(8.0);

            // ── Destinations ──────────────────────────────────────────────
            ui.label("Destinations:");

            let has_source = self
                .config
                .source_repo
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);

            for (i, dest) in self.config.destinations.iter().enumerate() {
                ui.horizontal(|ui| {
                    // Label + path take most of the row width
                    ui.add(
                        egui::Label::new(format!("📁 {}  —  {}", dest.label, dest.path)).truncate(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("❌").clicked() {
                            action = Some(UiAction::RemoveDest(i));
                        }
                        if ui
                            .add_enabled(has_source, egui::Button::new("⚡").small())
                            .clicked()
                        {
                            action = Some(UiAction::ForceSyncOne(i));
                        }
                        if ui
                            .add_enabled(has_source, egui::Button::new("🔄").small())
                            .clicked()
                        {
                            action = Some(UiAction::SyncOne(i));
                        }
                    });
                });
            }

            // Add destination row
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_dest_label)
                        .hint_text("Label (e.g. Server 1)")
                        .desired_width(160.0),
                );
                if ui.button("📂 Pick folder…").clicked() && !self.new_dest_label.is_empty() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        action = Some(UiAction::AddDest {
                            label: self.new_dest_label.drain(..).collect(),
                            path: path.to_string_lossy().into_owned(),
                        });
                    }
                }
                if self.new_dest_label.is_empty() {
                    ui.weak("← enter a label first");
                }
            });

            ui.add_space(8.0);
            ui.separator();

            // ── Changed Files Preview ─────────────────────────────────────
            ui.label("Worktree changes to sync:");

            if self.changed_files.is_empty() {
                ui.weak("No worktree changes (or no repo selected)");
            } else {
                let total = self.changed_files.len();
                let show_count = if self.show_all_files {
                    total
                } else {
                    MAX_FILES_SHOWN.min(total)
                };

                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for file in self.changed_files.iter().take(show_count) {
                            let icon = match file.status {
                                crate::git::FileStatus::Added => "+",
                                crate::git::FileStatus::Deleted => "-",
                                crate::git::FileStatus::Modified => "~",
                            };
                            ui.label(format!("{} {}", icon, file.relative_path));
                        }
                    });

                if total > MAX_FILES_SHOWN && !self.show_all_files {
                    ui.horizontal(|ui| {
                        ui.weak(format!("...and {} more files", total - MAX_FILES_SHOWN));
                        if ui.small_button("Show all").clicked() {
                            self.show_all_files = true;
                        }
                    });
                }
            }

            ui.add_space(8.0);
            ui.separator();

            // ── Global Action Buttons ─────────────────────────────────────
            let can_act = has_source && !self.config.destinations.is_empty();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_act, egui::Button::new("🔄 Sync All"))
                    .clicked()
                {
                    action = Some(UiAction::SyncAll);
                }
                if ui
                    .add_enabled(can_act, egui::Button::new("⚡ Force Sync All"))
                    .clicked()
                {
                    action = Some(UiAction::ForceSyncAll);
                }
            });

            if let Some(ref msg) = self.status_message {
                ui.add_space(4.0);
                ui.label(msg);
            }
        });

        // ── Execute deferred actions ───────────────────────────────────────
        // Now that the UI borrow is released we can call &mut self methods freely.
        match action {
            Some(UiAction::SetSource(p)) => {
                self.config.source_repo = Some(p);
                self.refresh_changed_files();
                let _ = self.config.save();
            }
            Some(UiAction::ClearSource) => {
                self.config.source_repo = None;
                self.changed_files.clear();
                let _ = self.config.save();
            }
            Some(UiAction::AddDest { label, path }) => {
                self.config.destinations.push(Destination { label, path });
                let _ = self.config.save();
            }
            Some(UiAction::RemoveDest(i)) => {
                self.config.destinations.remove(i);
                let _ = self.config.save();
            }
            Some(UiAction::SyncOne(i)) => self.run_sync_one(i),
            Some(UiAction::ForceSyncOne(i)) => self.run_force_sync_one(i),
            Some(UiAction::SyncAll) => self.run_sync(),
            Some(UiAction::ForceSyncAll) => self.run_force_sync(),
            None => {}
        }
    }
}

/// All things the UI can request — collected during the frame, executed after.
///
/// WHY an enum instead of calling methods directly in the closure?
/// egui's `show` closure borrows `ui` (and transitively `ctx`) mutably.
/// Calling `self.run_sync()` inside would create a second mutable borrow of `self`
/// at the same time — the borrow checker rejects this. Recording intent as an enum
/// value and acting on it after the closure returns is the idiomatic egui solution.
enum UiAction {
    SetSource(String),
    ClearSource,
    AddDest { label: String, path: String },
    RemoveDest(usize),
    SyncOne(usize),
    ForceSyncOne(usize),
    SyncAll,
    ForceSyncAll,
}
