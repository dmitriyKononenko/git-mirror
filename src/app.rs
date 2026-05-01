// app.rs — egui application state and UI rendering

use crate::config::{Config, CopyGroup};
use eframe::egui;
use rfd::FileDialog;

pub struct CopyApp {
    config: Config,
    // Per-group status message (index matches config.groups)
    group_status: Vec<Option<String>>,
    global_status: Option<String>,
}

impl CopyApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load().unwrap_or_default();
        let group_count = config.groups.len();
        Self {
            config,
            group_status: vec![None; group_count],
            global_status: None,
        }
    }

    fn copy_group(group: &CopyGroup) -> anyhow::Result<()> {
        let src = std::path::Path::new(&group.source);
        let dst = std::path::Path::new(&group.destination);
        if src.is_dir() {
            let opts = fs_extra::dir::CopyOptions::new()
                .overwrite(true)
                .copy_inside(true);
            fs_extra::dir::copy(src, dst, &opts)?;
        } else {
            let file_name = src
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Source has no file name"))?;
            let opts = fs_extra::file::CopyOptions::new().overwrite(true);
            fs_extra::file::copy(src, dst.join(file_name), &opts)?;
        }
        Ok(())
    }

    fn ensure_status_len(&mut self) {
        let n = self.config.groups.len();
        self.group_status.resize(n, None);
    }
}

impl eframe::App for CopyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Deferred actions to avoid borrow-checker conflicts inside egui closures
        let mut action: Option<UiAction> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Copy Automation");
            ui.separator();
            ui.add_space(4.0);

            // ── Copy groups ───────────────────────────────────────────────
            for (i, group) in self.config.groups.iter().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Source:");
                        let src_display = if group.source.is_empty() {
                            "Not selected…".to_owned()
                        } else {
                            group.source.clone()
                        };
                        ui.add(
                            egui::TextEdit::singleline(&mut src_display.as_str())
                                .desired_width(300.0),
                        );
                        if ui.button("Browse…").clicked() {
                            action = Some(UiAction::PickSource(i));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Destination:");
                        let dst_display = if group.destination.is_empty() {
                            "Not selected…".to_owned()
                        } else {
                            group.destination.clone()
                        };
                        ui.add(
                            egui::TextEdit::singleline(&mut dst_display.as_str())
                                .desired_width(300.0),
                        );
                        if ui.button("Browse…").clicked() {
                            action = Some(UiAction::PickDestination(i));
                        }
                    });
                    ui.horizontal(|ui| {
                        let can_copy = !group.source.is_empty() && !group.destination.is_empty();
                        if ui
                            .add_enabled(can_copy, egui::Button::new("Copy"))
                            .clicked()
                        {
                            action = Some(UiAction::CopyOne(i));
                        }
                        if ui.button("Delete").clicked() {
                            action = Some(UiAction::RemoveGroup(i));
                        }
                        if let Some(Some(ref msg)) = self.group_status.get(i) {
                            ui.label(msg);
                        }
                    });
                });
                ui.add_space(4.0);
            }

            // ── Toolbar ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                if ui.button("+ Add").clicked() {
                    action = Some(UiAction::AddGroup);
                }
                let can_copy_all = self
                    .config
                    .groups
                    .iter()
                    .any(|g| !g.source.is_empty() && !g.destination.is_empty());
                if ui
                    .add_enabled(can_copy_all, egui::Button::new("Copy All"))
                    .clicked()
                {
                    action = Some(UiAction::CopyAll);
                }
            });

            if let Some(ref msg) = self.global_status {
                ui.add_space(4.0);
                ui.label(msg);
            }
        });

        // ── Execute deferred actions ───────────────────────────────────────
        match action {
            Some(UiAction::AddGroup) => {
                self.config.groups.push(CopyGroup {
                    source: String::new(),
                    destination: String::new(),
                });
                self.ensure_status_len();
                let _ = self.config.save();
            }
            Some(UiAction::RemoveGroup(i)) => {
                self.config.groups.remove(i);
                self.group_status.remove(i);
                let _ = self.config.save();
            }
            Some(UiAction::PickSource(i)) => {
                // Try picking a file; if user cancels, fall back to folder picker
                let path = FileDialog::new()
                    .pick_file()
                    .or_else(|| FileDialog::new().pick_folder());
                if let Some(p) = path {
                    if let Some(g) = self.config.groups.get_mut(i) {
                        g.source = p.to_string_lossy().into_owned();
                    }
                    let _ = self.config.save();
                }
            }
            Some(UiAction::PickDestination(i)) => {
                if let Some(path) = FileDialog::new().pick_folder() {
                    if let Some(g) = self.config.groups.get_mut(i) {
                        g.destination = path.to_string_lossy().into_owned();
                    }
                    let _ = self.config.save();
                }
            }
            Some(UiAction::CopyOne(i)) => {
                if let Some(group) = self.config.groups.get(i) {
                    let result = Self::copy_group(group);
                    self.ensure_status_len();
                    self.group_status[i] = Some(match result {
                        Ok(()) => "✓ Copied".into(),
                        Err(e) => format!("✗ {}", e),
                    });
                }
            }
            Some(UiAction::CopyAll) => {
                let mut ok = 0usize;
                let mut fail = 0usize;
                self.ensure_status_len();
                for (i, group) in self.config.groups.iter().enumerate() {
                    if group.source.is_empty() || group.destination.is_empty() {
                        continue;
                    }
                    match Self::copy_group(group) {
                        Ok(()) => {
                            self.group_status[i] = Some("✓ Copied".into());
                            ok += 1;
                        }
                        Err(e) => {
                            self.group_status[i] = Some(format!("✗ {}", e));
                            fail += 1;
                        }
                    }
                }
                self.global_status = Some(if fail == 0 {
                    format!("✓ All done — {} group(s) copied", ok)
                } else {
                    format!("Done — {} ok, {} failed", ok, fail)
                });
            }
            None => {}
        }
    }
}

enum UiAction {
    AddGroup,
    RemoveGroup(usize),
    PickSource(usize),
    PickDestination(usize),
    CopyOne(usize),
    CopyAll,
}
