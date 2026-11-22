//! Opening, creating, copying and switching projects; the welcome screen.

use super::*;

/// Name of the system file manager, for the project menu.
pub(super) const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in File Manager"
};

/// Opens `dir` in Finder / Explorer / the desktop's file manager. (A file:// URL would go
/// to the default web browser instead.)
fn reveal_in_file_manager(dir: &Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(|_| ())
}

impl App {
    /// Opens (or creates) the project in `dir`. An old sweeps.json there is imported into
    /// the new database the first time.
    pub(super) fn open_project(&mut self, ctx: &egui::Context, dir: &Path) {
        // Reopening the same folder: release its lock first. Otherwise open the new one
        // before letting go of the current one, so a failure (e.g. it's open in another
        // window) leaves you where you were.
        if self.project.as_deref() == Some(dir) {
            self.store = None;
        }
        let store = match Store::open(dir) {
            Ok(store) => store,
            Err(e) => {
                self.status = Some(format!("Couldn't open {}: {e}", project_name(dir)));
                if self.store.is_none() {
                    self.close_project(ctx);
                }
                return;
            }
        };
        self.status = store
            .backup(dir)
            .err()
            .map(|e| format!("Couldn't back up the project: {e}"));
        self.store = Some(store);
        self.project = Some(dir.to_path_buf());
        self.reload();
        self.config.push_recent(dir);
        let _ = self.config.save();
        self.reset_view();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — Sweeps Tracker",
            project_name(dir)
        )));
    }

    pub(super) fn close_project(&mut self, ctx: &egui::Context) {
        self.project = None;
        self.store = None;
        self.data = Data::default();
        self.reset_view();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Sweeps Tracker".into()));
    }

    pub(super) fn run_action(&mut self, ctx: &egui::Context, action: ProjectAction) {
        match action {
            ProjectAction::New => {
                let picked = rfd::FileDialog::new()
                    .set_title("Choose a folder for the new project")
                    .pick_folder();
                if let Some(dir) = picked {
                    let existed = store::is_project(&dir);
                    self.open_project(ctx, &dir);
                    if existed && self.status.is_none() {
                        self.status =
                            Some("That folder already had a project, so it was opened.".into());
                    }
                }
            }
            ProjectAction::Open => {
                let picked = rfd::FileDialog::new()
                    .set_title("Open a project folder")
                    .pick_folder();
                if let Some(dir) = picked {
                    let create = store::is_project(&dir)
                        || rfd::MessageDialog::new()
                            .set_title("No project here")
                            .set_description(format!(
                                "\"{}\" doesn't contain a Sweeps Tracker project. Create a new one in this folder?",
                                project_name(&dir)
                            ))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::Yes;
                    if create {
                        self.open_project(ctx, &dir);
                    }
                }
            }
            ProjectAction::OpenRecent(dir) => {
                if store::is_project(&dir) {
                    self.open_project(ctx, &dir);
                } else {
                    self.config.recent.retain(|d| d != &dir);
                    let _ = self.config.save();
                    self.status = Some(format!(
                        "{} no longer has a project, so it was removed from Recent.",
                        dir.display()
                    ));
                }
            }
            ProjectAction::SaveAs => {
                let picked = rfd::FileDialog::new()
                    .set_title("Save a copy of this project to a folder")
                    .pick_folder();
                if let Some(dir) = picked {
                    let overwrite = !store::is_project(&dir)
                        || rfd::MessageDialog::new()
                            .set_title("Replace project?")
                            .set_description(format!(
                                "\"{}\" already has a project. Replace it with this one?",
                                project_name(&dir)
                            ))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::Yes;
                    if self.project.as_deref() == Some(dir.as_path()) {
                        self.status = Some("That's the folder this project is already in.".into());
                    } else if overwrite && store::in_use(&dir) {
                        self.status = Some(format!(
                            "\"{}\" is open in another Sweeps Tracker window; close it there first.",
                            project_name(&dir)
                        ));
                    } else if overwrite {
                        // VACUUM INTO needs a path that doesn't exist yet.
                        let target = store::db_path(&dir);
                        let copied = std::fs::remove_file(&target)
                            .or_else(|e| match e.kind() {
                                std::io::ErrorKind::NotFound => Ok(()),
                                _ => Err(e),
                            })
                            .map_err(|e| e.to_string())
                            .and_then(|_| self.db().copy_to(&target));
                        match copied {
                            Ok(()) => self.open_project(ctx, &dir),
                            Err(e) => self.status = Some(format!("Couldn't save the copy: {e}")),
                        }
                    }
                }
            }
            ProjectAction::Reveal => {
                if let Some(dir) = &self.project {
                    if let Err(e) = reveal_in_file_manager(dir) {
                        self.status = Some(format!("Couldn't open the folder: {e}"));
                    }
                }
            }
            ProjectAction::Close => self.close_project(ctx),
        }
    }

    /// Sidebar project switcher: a card showing the open project that drops down a menu of
    /// project actions and recent projects (like VS Code's folder picker).
    pub(super) fn project_switcher(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self.project.clone() else {
            return;
        };
        let resp =
            theme::project_button(ui, &project_name(&dir)).on_hover_text(dir.display().to_string());
        let popup_id = ui.make_persistent_id("project_menu");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }
        let shortcut = |mods: egui::Modifiers, key: Key| {
            ui.ctx()
                .format_shortcut(&egui::KeyboardShortcut::new(mods, key))
        };
        let (new_keys, open_keys, save_as_keys) = (
            shortcut(egui::Modifiers::COMMAND, Key::N),
            shortcut(egui::Modifiers::COMMAND, Key::O),
            shortcut(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::S),
        );
        let mut action = None;
        egui::popup::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::popup::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if theme::menu_item(ui, "New Project…", &new_keys).clicked() {
                    action = Some(ProjectAction::New);
                }
                if theme::menu_item(ui, "Open Folder…", &open_keys).clicked() {
                    action = Some(ProjectAction::Open);
                }
                if theme::menu_item(ui, "Save As…", &save_as_keys).clicked() {
                    action = Some(ProjectAction::SaveAs);
                }
                if theme::menu_item(ui, REVEAL_LABEL, "").clicked() {
                    action = Some(ProjectAction::Reveal);
                }
                if theme::menu_item(ui, "Close Project", "").clicked() {
                    action = Some(ProjectAction::Close);
                }
                let others: Vec<&PathBuf> =
                    self.config.recent.iter().filter(|d| **d != dir).collect();
                if !others.is_empty() {
                    ui.add_space(8.0);
                    ui.label(dim("  RECENT").small());
                    ui.add_space(2.0);
                    for d in others {
                        if theme::menu_item(ui, &project_name(d), "")
                            .on_hover_text(d.display().to_string())
                            .clicked()
                        {
                            action = Some(ProjectAction::OpenRecent(d.clone()));
                        }
                    }
                }
            },
        );
        if action.is_some() {
            ui.memory_mut(|m| m.close_popup());
            self.action = action;
        }
    }

    /// Shown when no project is open: create/open buttons and recent projects.
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.12);
            ui.set_max_width(460.0);
            ui.label(
                RichText::new("SWEEPS TRACKER")
                    .size(30.0)
                    .family(theme::bold()),
            );
            ui.label(dim(
                "Each project is a folder with its own casinos and entries.",
            ));
            ui.add_space(20.0);
            ui.columns(2, |cols| {
                let w = cols[0].available_width() - theme::SHADOW;
                if theme::primary_button(&mut cols[0], true, "New Project…", w).clicked() {
                    self.action = Some(ProjectAction::New);
                }
                let w = cols[1].available_width() - theme::SHADOW;
                if theme::shadowed(&mut cols[1], |ui| {
                    ui.add(egui::Button::new("Open Folder…").min_size(Vec2::new(w, 0.0)))
                })
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                {
                    self.action = Some(ProjectAction::Open);
                }
            });
            if !self.config.recent.is_empty() {
                ui.add_space(28.0);
                theme::section(ui, "Recent projects");
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                for dir in &self.config.recent {
                    if theme::list_item(ui, &project_name(dir), false, None, None)
                        .on_hover_text(dir.display().to_string())
                        .clicked()
                    {
                        self.action = Some(ProjectAction::OpenRecent(dir.clone()));
                    }
                }
            }
        });
    }
}
