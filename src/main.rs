// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod grail;
mod model;
mod store;
mod theme;
mod titlebar;

use chrono::{Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, RichText, Sense, Stroke, Vec2,
};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use grail::{GrailCasino, GrailDb};
use model::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use store::Store;
use theme::{ACCENT, DIM, GAIN, LOSS, PRIMARY};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: titlebar::configure(
            egui::ViewportBuilder::default()
                .with_title("Sweeps Tracker")
                .with_inner_size([1150.0, 720.0])
                .with_min_inner_size([820.0, 520.0]),
        ),
        ..Default::default()
    };
    eframe::run_native(
        "Sweeps Tracker",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx)))
        }),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Overview,
    Casino(i64),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Purchase,
    Withdrawal,
}

struct EntryForm {
    kind: Kind,
    date: NaiveDate,
    spent: String,
    cc: String,
    amount: String,
    note: String,
    focus_first: bool,
}

/// Which row an edit / delete applies to.
#[derive(Clone, Copy, PartialEq)]
enum RowRef {
    Purchase(i64),
    Redemption(i64),
}

/// Edit dialog state. Purchases use `spent` + `cc`; redemptions use `amount`.
struct EditState {
    row: RowRef,
    date: NaiveDate,
    spent: String,
    cc: String,
    amount: String,
    note: String,
}

/// The "Add casino" dropdown.
struct Picker {
    open: bool,
    query: String,
    /// Focus the search box on the frame after opening.
    focus: bool,
    /// Screen rect of the button that opens it; the dropdown hangs below this.
    anchor: egui::Rect,
}

/// File-menu / shortcut / welcome-screen actions, run after the frame's UI is drawn
/// (the folder pickers are blocking native dialogs).
enum ProjectAction {
    New,
    Open,
    OpenRecent(PathBuf),
    SaveAs,
    Reveal,
    Close,
}

struct App {
    /// In-memory copy of the open project, reloaded from `store` after every change.
    data: Data,
    /// Open project folder (holds sweeps.db and backups/). None shows the welcome screen.
    project: Option<PathBuf>,
    /// Database of the open project (Some whenever `project` is).
    store: Option<Store>,
    config: Config,
    action: Option<ProjectAction>,
    view: View,
    picker: Picker,
    /// Casino directory from SweepsGrail (cached on disk, refreshed daily).
    grail: GrailDb,
    grail_rx: Option<Receiver<Result<GrailDb, String>>>,
    grail_error: Option<String>,
    form: EntryForm,
    editing: Option<EditState>,
    pasting: Option<(i64, String)>,
    renaming: Option<(i64, String)>,
    confirm_delete: Option<i64>,
    status: Option<String>,
}

/// Date picker button styled for the app: no red weekend highlighting (egui hard-codes
/// that color) and no week-number column.
fn date_picker<'a>(date: &'a mut NaiveDate, id: &'a str) -> DatePickerButton<'a> {
    DatePickerButton::new(date)
        .id_salt(id)
        .highlight_weekends(false)
        .calendar_week(false)
}

/// Name of the system file manager, for the project menu.
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
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

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn profit_color(v: Cents) -> Color32 {
    if v > 0 {
        GAIN
    } else if v < 0 {
        LOSS
    } else {
        PRIMARY
    }
}

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text).color(DIM)
}

/// Amount text field; returns its response so callers can detect Enter.
fn amount_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    let invalid = !text.trim().is_empty() && parse_cents(text).is_none_or(|v| v < 0);
    let mut edit = egui::TextEdit::singleline(text).hint_text(hint);
    if invalid {
        edit = edit.text_color(LOSS);
    }
    theme::input(ui, edit, width)
}

/// Padding inside the flush grid cells of the totals and entry boxes.
const CELL_PAD: Vec2 = Vec2::new(12.0, 8.0);

/// Width that fills the cell, leaving a little room for the focus glow / button shadow.
fn fill_width(ui: &egui::Ui) -> f32 {
    ui.available_width() - theme::SHADOW
}

/// Grid cell in the entry box's field row: small label above the field.
fn field_cell<R>(
    ui: &mut egui::Ui,
    first_col: bool,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    theme::grid_cell(ui, first_col, false, None, CELL_PAD, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(dim(label).small());
        add(ui)
    })
    .inner
}

fn submitted(ui: &egui::Ui, r: &egui::Response) -> bool {
    r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

/// One cell of the totals grid. The net profit cell is tinted green/red when non-zero.
fn stat_cell(
    ui: &mut egui::Ui,
    first_col: bool,
    title: &str,
    value: String,
    detail: Option<String>,
    color: Color32,
) {
    let tint = (color == GAIN || color == LOSS).then(|| color.gamma_multiply(0.1));
    theme::grid_cell(ui, first_col, true, tint, CELL_PAD, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 2.0);
        ui.label(dim(title).small());
        // Detail (e.g. "+15 bonus") sits beside the value so every cell is one height.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(value)
                    .size(24.0)
                    .family(theme::bold())
                    .color(color),
            );
            if let Some(detail) = detail {
                ui.label(dim(detail).small());
            }
        });
    });
}

/// Totals as one flush grid box (same style as the SweepsGrail details), with a
/// "recovered" progress row underneath once there's spending.
fn stats_row(ui: &mut egui::Ui, stats: Stats) {
    theme::boxed_flush(ui, |ui| {
        ui.columns(4, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().item_spacing = Vec2::ZERO;
            }
            let bonus =
                (stats.bonus_coins() > 0).then(|| format!("+{} bonus", coins(stats.bonus_coins())));
            stat_cell(
                &mut cols[0],
                true,
                "SPENT",
                money(stats.spent),
                None,
                PRIMARY,
            );
            stat_cell(
                &mut cols[1],
                false,
                "CC RECEIVED",
                coins(stats.cc_received),
                bonus,
                PRIMARY,
            );
            stat_cell(
                &mut cols[2],
                false,
                "WITHDRAWN",
                money(stats.withdrawn),
                None,
                PRIMARY,
            );
            stat_cell(
                &mut cols[3],
                false,
                "NET PROFIT",
                signed_money(stats.net()),
                None,
                profit_color(stats.net()),
            );
        });
        // How much of what was spent has come back out, drawn like a playback progress bar.
        if stats.spent > 0 {
            let recovered = stats.withdrawn as f64 / stats.spent as f64;
            theme::grid_cell(ui, true, false, None, CELL_PAD, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.label(dim("RECOVERED").small());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} of spend", percent(recovered))).small());
                    });
                });
                theme::progress(ui, recovered as f32);
            });
        }
    });
    ui.add_space(theme::SHADOW);
}

fn page_header(ui: &mut egui::Ui, title: &str, add_right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(Layout::right_to_left(Align::Center), add_right);
    });
    theme::underline(ui);
    ui.add_space(12.0);
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let config = Config::load();
        // Reopen the most recent project that still exists; on the very first run, bring
        // over data saved by the pre-project version.
        let start = config
            .recent
            .iter()
            .find(|d| store::is_project(d))
            .cloned()
            .or_else(|| {
                if config.recent.is_empty() {
                    migrate_legacy_data()
                } else {
                    None
                }
            });
        let grail = GrailDb::load(&GrailDb::path());
        let grail_rx = grail
            .is_stale()
            .then(|| grail::fetch_in_background(ctx.clone(), GrailDb::path()));
        let mut app = App {
            data: Data::default(),
            project: None,
            store: None,
            config,
            action: None,
            view: View::Overview,
            picker: Picker {
                open: false,
                query: String::new(),
                focus: false,
                anchor: egui::Rect::NOTHING,
            },
            grail,
            grail_rx,
            grail_error: None,
            form: EntryForm {
                kind: Kind::Purchase,
                date: today(),
                spent: String::new(),
                cc: String::new(),
                amount: String::new(),
                note: String::new(),
                focus_first: false,
            },
            editing: None,
            pasting: None,
            renaming: None,
            confirm_delete: None,
            status: None,
        };
        if let Some(dir) = start {
            app.open_project(ctx, &dir);
        }
        app
    }

    /// The open project's database. Only called while a project is open.
    fn db(&mut self) -> &mut Store {
        self.store.as_mut().expect("a project is open")
    }

    /// Reloads the in-memory data after a database change.
    fn reload(&mut self) {
        let Some(store) = &self.store else { return };
        match store.load() {
            Ok(data) => self.data = data,
            Err(e) => self.status = Some(format!("Couldn't load the project: {e}")),
        }
    }

    /// Runs a database change: reloads on success, shows the error otherwise.
    fn apply<T>(&mut self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(v) => {
                self.reload();
                Some(v)
            }
            Err(e) => {
                self.status = Some(e);
                None
            }
        }
    }

    /// Opens (or creates) the project in `dir`. An old sweeps.json there is imported into
    /// the new database the first time.
    fn open_project(&mut self, ctx: &egui::Context, dir: &Path) {
        // Release the current project's lock first (it may be the same folder).
        self.store = None;
        let store = match Store::open(dir) {
            Ok(store) => store,
            Err(e) => {
                self.status = Some(format!("Couldn't open {}: {e}", project_name(dir)));
                self.close_project(ctx);
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

    fn close_project(&mut self, ctx: &egui::Context) {
        self.project = None;
        self.store = None;
        self.data = Data::default();
        self.reset_view();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("Sweeps Tracker".into()));
    }

    /// Clears per-project UI state when switching projects.
    fn reset_view(&mut self) {
        self.view = View::Overview;
        self.picker.open = false;
        self.editing = None;
        self.pasting = None;
        self.renaming = None;
        self.confirm_delete = None;
    }

    fn run_action(&mut self, ctx: &egui::Context, action: ProjectAction) {
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
    fn project_switcher(&mut self, ui: &mut egui::Ui) {
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
    fn welcome(&mut self, ui: &mut egui::Ui) {
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

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        self.project_switcher(ui);
        ui.add_space(12.0);

        if theme::list_item(ui, "Overview", self.view == View::Overview, None, None).clicked() {
            self.view = View::Overview;
        }
        ui.add_space(10.0);
        theme::section(ui, "Casinos");

        let full = Vec2::new(ui.available_width() - theme::SHADOW, 0.0);
        let r = theme::shadowed(ui, |ui| {
            ui.add(egui::Button::new("+  Add casino").min_size(full))
        })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() {
            self.picker.open = !self.picker.open;
            self.picker.query.clear();
            self.picker.focus = true;
        }
        self.picker.anchor = r.rect;
        ui.add_space(10.0);

        // Rows are button-style cards now, so the list doesn't need its own box around it.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                {
                    for casino in &self.data.casinos {
                        let selected = self.view == View::Casino(casino.id);
                        let trailing = (casino.entry_count() > 0).then(|| {
                            let net = casino.stats().net();
                            (signed_money(net), profit_color(net))
                        });
                        let logo = self.grail.find(&casino.name).map(|g| g.logo.as_str());
                        if theme::list_item(ui, &casino.name, selected, logo, trailing).clicked() {
                            self.view = View::Casino(casino.id);
                        }
                    }
                }
            });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Overview", |_| {});
        stats_row(ui, self.data.stats());
        ui.add_space(16.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::section(ui, "Profit over time");
                ui.add_space(4.0);
                profit_chart(ui, self.data.daily_net());
                ui.add_space(20.0);

                theme::section(ui, "By casino");
                ui.add_space(4.0);
                let mut rows: Vec<(i64, &str, Stats, usize)> = self
                    .data
                    .casinos
                    .iter()
                    .map(|c| (c.id, c.name.as_str(), c.stats(), c.entry_count()))
                    .collect();
                rows.sort_by(|a, b| b.2.net().cmp(&a.2.net()));

                // Flush grid with equal-width columns, so it always fits the box (a Grid with
                // minimum column widths ran off the right edge in narrow windows).
                let mut open = None;
                let pad = Vec2::new(10.0, 6.0);
                theme::boxed_flush(ui, |ui| {
                    let headers = [
                        "CASINO",
                        "ENTRIES",
                        "SPENT",
                        "CC RECEIVED",
                        "DISCOUNT",
                        "WITHDRAWN",
                        "NET",
                    ];
                    ui.columns(headers.len(), |cols| {
                        for (c, (col, h)) in cols.iter_mut().zip(headers).enumerate() {
                            col.spacing_mut().item_spacing = Vec2::ZERO;
                            theme::grid_cell(col, c == 0, true, None, pad, |ui| {
                                ui.add(egui::Label::new(dim(h).small()).truncate());
                            });
                        }
                    });
                    if rows.is_empty() {
                        theme::grid_cell(ui, true, false, None, Vec2::new(10.0, 12.0), |ui| {
                            ui.label(dim("No casinos yet. Add one from the sidebar."));
                        });
                    }
                    for (r, (id, name, s, count)) in rows.into_iter().enumerate() {
                        let fill = (r % 2 == 1).then_some(theme::STRIPE);
                        let cells = [
                            count.to_string(),
                            money(s.spent),
                            coins(s.cc_received),
                            s.avg_discount().map(percent).unwrap_or_else(|| "—".into()),
                            money(s.withdrawn),
                            signed_money(s.net()),
                        ];
                        ui.columns(cells.len() + 1, |cols| {
                            for col in cols.iter_mut() {
                                col.spacing_mut().item_spacing = Vec2::ZERO;
                            }
                            theme::grid_cell(&mut cols[0], true, false, fill, pad, |ui| {
                                let name_text =
                                    RichText::new(name).family(theme::bold()).underline();
                                let link = ui
                                    .add(
                                        egui::Label::new(name_text)
                                            .truncate()
                                            .sense(Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if link.clicked() {
                                    open = Some(id);
                                }
                            });
                            let last = cells.len();
                            for (c, (col, text)) in cols[1..].iter_mut().zip(cells).enumerate() {
                                let color = if c + 1 == last {
                                    profit_color(s.net())
                                } else {
                                    PRIMARY
                                };
                                theme::grid_cell(col, false, false, fill, pad, |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(text).color(color))
                                            .truncate(),
                                    );
                                });
                            }
                        });
                    }
                });
                ui.add_space(theme::SHADOW + 4.0);
                if let Some(id) = open {
                    self.view = View::Casino(id);
                }
            });
    }

    fn casino_view(&mut self, ui: &mut egui::Ui, id: i64) {
        let Some(casino) = self.data.casino(id) else {
            self.view = View::Overview;
            return;
        };

        let info = self.grail.find(&casino.name);
        // Buttons are laid out first (from the right) so they always get their space; the
        // logo, name and tier then fill what's left, and a long name truncates instead of
        // running under the buttons.
        let header = Vec2::new(ui.available_width(), 44.0);
        ui.allocate_ui_with_layout(header, Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(theme::SHADOW);
            if theme::button(ui, RichText::new("Delete").color(LOSS))
                .on_hover_text("Delete this casino and all its entries")
                .clicked()
            {
                self.confirm_delete = Some(id);
            }
            if theme::button(ui, "Rename").clicked() {
                self.renaming = Some((id, casino.name.clone()));
            }
            if theme::button(ui, "Paste rows")
                .on_hover_text("Import rows copied from Excel or Google Sheets")
                .clicked()
            {
                self.pasting = Some((id, String::new()));
            }
            if let Some(g) = info.filter(|g| !g.review_url.is_empty()) {
                if theme::button(ui, "Review")
                    .on_hover_text("Open the full review on SweepsGrail")
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&g.review_url));
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if let Some(g) = info {
                    theme::logo(ui, &g.logo, 40.0);
                }
                // Keep room for the tier badge and NEW tag after the name.
                let reserve = if info.is_some() { 90.0 } else { 0.0 };
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - reserve).max(60.0));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&casino.name).text_style(egui::TextStyle::Heading),
                        )
                        .truncate(),
                    );
                });
                if let Some(g) = info {
                    if !g.tier.is_empty() {
                        theme::tier_badge(ui, &g.tier, 26.0);
                    }
                    if g.is_new {
                        theme::tag(ui, "NEW", GAIN);
                    }
                }
            });
        });
        theme::underline(ui);
        ui.add_space(10.0);
        if let Some(g) = info {
            grail_info(ui, g);
            ui.add_space(8.0);
        }
        stats_row(ui, casino.stats());
        ui.add_space(10.0);

        self.add_entry_form(ui, id);
        ui.add_space(10.0);
        self.entries_table(ui, id);
    }

    fn add_entry_form(&mut self, ui: &mut egui::Ui, casino_id: i64) {
        let f = &mut self.form;
        let mut submit = false;

        // Flush grid like the boxes above: a top cell with the type tabs and date, then a row
        // of cells (label above field) ending with the Add button.
        // Blank fields count as 0; anything typed must be a non-negative amount.
        let ok = |t: &str| t.trim().is_empty() || parse_cents(t).is_some_and(|v| v >= 0);
        let cents = |t: &str| parse_cents(t).unwrap_or(0);
        let valid = match f.kind {
            Kind::Purchase => {
                ok(&f.spent) && ok(&f.cc) && (cents(&f.spent) > 0 || cents(&f.cc) > 0)
            }
            Kind::Withdrawal => cents(&f.amount) > 0,
        };
        theme::boxed_flush(ui, |ui| {
            theme::grid_cell(ui, true, true, None, CELL_PAD, |ui| {
                // The flush grid zeroes spacing; this row needs normal gaps between widgets.
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 6.0);
                ui.horizontal(|ui| {
                    let mut tab = if f.kind == Kind::Purchase { 0 } else { 1 };
                    theme::tab_bar(
                        ui,
                        "entry_kind",
                        &mut tab,
                        &["Purchase", "Withdrawal"],
                        240.0,
                    );
                    f.kind = if tab == 0 {
                        Kind::Purchase
                    } else {
                        Kind::Withdrawal
                    };
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(theme::SHADOW);
                        if theme::button(ui, "Today").clicked() {
                            f.date = today();
                        }
                        theme::shadowed(ui, |ui| {
                            ui.add(date_picker(&mut f.date, "new_entry_date"))
                        });
                        ui.label(dim("DATE").small());
                    });
                });
            });

            let purchase = f.kind == Kind::Purchase;
            ui.columns(if purchase { 5 } else { 3 }, |cols| {
                for col in cols.iter_mut() {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                }
                let first = if purchase {
                    let r = field_cell(&mut cols[0], true, "SPENT $", |ui| {
                        let w = fill_width(ui);
                        amount_field(ui, &mut f.spent, "85", w)
                    });
                    submit |= submitted(&cols[0], &r);
                    let r2 = field_cell(&mut cols[1], false, "CC RECEIVED", |ui| {
                        let w = fill_width(ui);
                        amount_field(ui, &mut f.cc, "100", w)
                    });
                    submit |= submitted(&cols[1], &r2);
                    let discount = match (cents(&f.spent), cents(&f.cc)) {
                        (s, c) if s > 0 && c > 0 => percent((c - s) as f64 / c as f64),
                        _ => "—".into(),
                    };
                    field_cell(&mut cols[2], false, "DISCOUNT", |ui| {
                        let size = [ui.available_width(), ui.spacing().interact_size.y];
                        ui.add_sized(
                            size,
                            egui::Label::new(RichText::new(discount).family(theme::bold())),
                        )
                    });
                    r
                } else {
                    let r = field_cell(&mut cols[0], true, "AMOUNT $", |ui| {
                        let w = fill_width(ui);
                        amount_field(ui, &mut f.amount, "500", w)
                    });
                    submit |= submitted(&cols[0], &r);
                    r
                };
                if f.focus_first {
                    first.request_focus();
                    f.focus_first = false;
                }

                let (note_col, add_col) = if purchase { (3, 4) } else { (1, 2) };
                let r = field_cell(&mut cols[note_col], false, "NOTE", |ui| {
                    let w = fill_width(ui);
                    theme::input(
                        ui,
                        egui::TextEdit::singleline(&mut f.note).hint_text("optional"),
                        w,
                    )
                });
                submit |= submitted(&cols[note_col], &r);
                // Blank label keeps the button level with the inputs beside it.
                let add = field_cell(&mut cols[add_col], false, " ", |ui| {
                    let w = fill_width(ui);
                    theme::primary_button(ui, valid, "Add entry", w)
                });
                submit |= add.clicked();
            });
        });
        submit &= valid;

        if !submit {
            return;
        }
        let (kind, date, note) = (f.kind, f.date, f.note.trim().to_string());
        let (spent, cc, amount) = (cents(&f.spent), cents(&f.cc), cents(&f.amount));
        let result = match kind {
            Kind::Purchase => self.db().add_purchase(casino_id, date, spent, cc, &note),
            Kind::Withdrawal => self.db().add_redemption(casino_id, date, amount, &note),
        };
        // Keep what was typed if saving failed; otherwise clear for the next entry.
        if self.apply(result).is_some() {
            let f = &mut self.form;
            f.spent.clear();
            f.cc.clear();
            f.amount.clear();
            f.note.clear();
            f.focus_first = true;
        }
    }

    fn entries_table(&mut self, ui: &mut egui::Ui, casino_id: i64) {
        let Some(casino) = self.data.casino(casino_id) else {
            return;
        };
        if casino.entry_count() == 0 {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(dim(
                    "No entries yet. Add one above, or paste rows from your spreadsheet.",
                ));
            });
            return;
        }

        // Purchases and redemptions in one list, newest first, like the spreadsheet.
        let mut rows: Vec<Row> = casino
            .purchases
            .iter()
            .map(Row::Purchase)
            .chain(casino.redemptions.iter().map(Row::Redemption))
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.sort_key()));

        let mut edit = None;
        let mut delete = None;
        let right = Layout::right_to_left(Align::Center);

        // Leave room below the table for the box's margin, border and shadow.
        let max_height =
            (ui.available_height() - 2.0 * (theme::MARGIN + theme::BORDER) - theme::SHADOW - 4.0)
                .max(80.0);
        theme::boxed(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .max_scroll_height(max_height)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(100.0))
                .column(Column::exact(100.0))
                .column(Column::exact(110.0))
                .column(Column::exact(90.0))
                .column(Column::exact(110.0))
                .column(Column::remainder().at_least(100.0))
                .column(Column::exact(110.0))
                .header(26.0, |mut header| {
                    for h in [
                        "DATE",
                        "SPENT",
                        "CC RECEIVED",
                        "DISCOUNT",
                        "WITHDRAWAL",
                        "NOTE",
                        "",
                    ] {
                        header.col(|ui| {
                            ui.label(dim(h).small());
                        });
                    }
                })
                .body(|mut body| {
                    for r in &rows {
                        body.row(34.0, |mut row| {
                            row.col(|ui| {
                                ui.label(r.date().format("%-m/%-d/%Y").to_string());
                            });
                            match r {
                                Row::Purchase(p) => {
                                    row.col(|ui| {
                                        if p.spent != 0 {
                                            ui.with_layout(right, |ui| ui.label(money(p.spent)));
                                        }
                                    });
                                    row.col(|ui| {
                                        if p.cc_received != 0 {
                                            ui.with_layout(right, |ui| {
                                                ui.label(coins(p.cc_received))
                                            });
                                        }
                                    });
                                    row.col(|ui| {
                                        if let Some(d) = p.discount() {
                                            ui.with_layout(right, |ui| ui.label(dim(percent(d))));
                                        }
                                    });
                                    row.col(|_| {});
                                }
                                Row::Redemption(red) => {
                                    for _ in 0..3 {
                                        row.col(|_| {});
                                    }
                                    row.col(|ui| {
                                        ui.with_layout(right, |ui| ui.label(money(red.amount)));
                                    });
                                }
                            }
                            row.col(|ui| {
                                ui.label(dim(r.note()));
                            });
                            row.col(|ui| {
                                if theme::button(ui, "Edit").clicked() {
                                    edit = Some(r.edit_state());
                                }
                                if theme::button(ui, RichText::new("✕").color(LOSS))
                                    .on_hover_text("Delete")
                                    .clicked()
                                {
                                    delete = Some(r.row_ref());
                                }
                            });
                        });
                    }
                });
        });

        if edit.is_some() {
            self.editing = edit;
        }
        if let Some(row) = delete {
            let result = match row {
                RowRef::Purchase(id) => self.db().delete_purchase(id),
                RowRef::Redemption(id) => self.db().delete_redemption(id),
            };
            self.apply(result);
        }
    }

    fn start_grail_fetch(&mut self, ctx: &egui::Context) {
        self.grail_error = None;
        self.grail_rx = Some(grail::fetch_in_background(ctx.clone(), GrailDb::path()));
    }

    /// Picks up the background SweepsGrail fetch once it finishes.
    fn poll_grail(&mut self) {
        let result = self.grail_rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("the download stopped unexpectedly".into()))
            }
        });
        match result {
            Some(Ok(db)) => {
                self.grail = db;
                self.grail_rx = None;
            }
            Some(Err(e)) => {
                self.grail_error = Some(e);
                self.grail_rx = None;
            }
            None => {}
        }
    }

    /// "Add casino" dropdown: search SweepsGrail's list, or add a custom name.
    fn casino_picker(&mut self, ctx: &egui::Context) {
        if !self.picker.open {
            return;
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut add: Option<String> = None;
        let mut open: Option<i64> = None;
        let mut refresh = false;

        let query = grail::normalize(&self.picker.query);
        let typed = self.picker.query.trim().to_string();
        // (SweepsGrail slug, your casino's id) for casinos already in your list.
        let added: Vec<(&str, i64)> = self
            .data
            .casinos
            .iter()
            .filter_map(|c| self.grail.find(&c.name).map(|g| (g.slug.as_str(), c.id)))
            .collect();
        let added_id = |slug: &str| added.iter().find(|(s, _)| *s == slug).map(|(_, id)| *id);
        let matches: Vec<&GrailCasino> = self
            .grail
            .casinos
            .iter()
            .filter(|g| query.is_empty() || grail::normalize(&g.name).contains(&query))
            .collect();
        let exact = !typed.is_empty()
            && (self.data.has_casino_named(&typed) || self.grail.find(&typed).is_some());
        let picker = &mut self.picker;
        let (loading, error, fetched_at) = (
            self.grail_rx.is_some(),
            &self.grail_error,
            self.grail.fetched_at,
        );

        let area = egui::Area::new(egui::Id::new("casino_picker"))
            .order(egui::Order::Foreground)
            .fixed_pos(picker.anchor.left_bottom() + Vec2::new(0.0, 10.0))
            .show(ctx, |ui| {
                theme::panel(PRIMARY).show(ui, |ui| {
                    ui.set_width(390.0);
                    let r = theme::input(
                        ui,
                        egui::TextEdit::singleline(&mut picker.query)
                            .hint_text("Search SweepsGrail casinos…"),
                        ui.available_width(),
                    );
                    if std::mem::take(&mut picker.focus) {
                        r.request_focus();
                    }
                    if submitted(ui, &r) {
                        // Enter adds the first match that isn't added yet, else the typed name.
                        add = matches
                            .iter()
                            .find(|g| added_id(&g.slug).is_none())
                            .map(|g| g.name.clone())
                            .or_else(|| (!typed.is_empty() && !exact).then(|| typed.clone()));
                    }
                    ui.add_space(8.0);
                    if !typed.is_empty() && !exact {
                        if theme::button(ui, format!("+  Add \"{typed}\" as a custom casino"))
                            .clicked()
                        {
                            add = Some(typed.clone());
                        }
                        ui.add_space(6.0);
                    }

                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            for g in &matches {
                                // Clicking a casino you already have opens it instead of adding it again.
                                let existing = added_id(&g.slug);
                                if picker_row(ui, g, existing.is_some()).clicked() {
                                    match existing {
                                        Some(id) => open = Some(id),
                                        None => add = Some(g.name.clone()),
                                    }
                                }
                            }
                            if matches.is_empty() && !loading {
                                ui.label(dim("No SweepsGrail casinos match."));
                            }
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if loading {
                            ui.spinner();
                            ui.label(dim("Loading casinos from SweepsGrail…").small());
                        } else if let Some(e) = error {
                            ui.label(
                                RichText::new(format!("Couldn't load SweepsGrail: {e}"))
                                    .small()
                                    .color(LOSS),
                            );
                        } else if fetched_at > 0 {
                            ui.label(
                                dim(format!(
                                    "{} casinos from SweepsGrail · updated {}",
                                    matches.len().max(added.len()),
                                    ago(fetched_at)
                                ))
                                .small(),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !loading && theme::button(ui, "Refresh").clicked() {
                                refresh = true;
                            }
                        });
                    });
                });
            });

        // Clicking anywhere outside the dropdown (or its button) closes it.
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !area.response.rect.contains(p) && !self.picker.anchor.contains(p)
                })
        });
        close |= outside;

        if refresh {
            self.start_grail_fetch(ctx);
        }
        if let Some(id) = open {
            self.view = View::Casino(id);
            close = true;
        }
        if let Some(name) = add {
            let result = self.db().add_casino(&name);
            if let Some(id) = self.apply(result) {
                self.view = View::Casino(id);
            }
            close = true;
        }
        if close {
            self.picker.open = false;
        }
    }

    /// Modal-ish dialogs: edit entry, paste rows, rename and delete casino.
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(ed) = &mut self.editing {
            let mut close = false;
            let mut save = false;
            let is_purchase = matches!(ed.row, RowRef::Purchase(_));
            let title = if is_purchase {
                "Edit purchase"
            } else {
                "Edit withdrawal"
            };
            dialog(ctx, title, |ui| {
                egui::Grid::new("edit_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(dim("DATE"));
                        ui.add(date_picker(&mut ed.date, "edit_date"));
                        ui.end_row();
                        if is_purchase {
                            ui.label(dim("SPENT $"));
                            amount_field(ui, &mut ed.spent, "0", 120.0);
                            ui.end_row();
                            ui.label(dim("CC RECEIVED"));
                            amount_field(ui, &mut ed.cc, "0", 120.0);
                            ui.end_row();
                        } else {
                            ui.label(dim("AMOUNT $"));
                            amount_field(ui, &mut ed.amount, "0", 120.0);
                            ui.end_row();
                        }
                        ui.label(dim("NOTE"));
                        theme::input(ui, egui::TextEdit::singleline(&mut ed.note), 200.0);
                        ui.end_row();
                    });
                let ok = |t: &str| t.trim().is_empty() || parse_cents(t).is_some_and(|v| v >= 0);
                let cents = |t: &str| parse_cents(t).unwrap_or(0);
                let valid = if is_purchase {
                    ok(&ed.spent) && ok(&ed.cc) && (cents(&ed.spent) > 0 || cents(&ed.cc) > 0)
                } else {
                    cents(&ed.amount) > 0
                };
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = theme::button_enabled(ui, valid, "Save").clicked();
                    close = theme::button(ui, "Cancel").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
            if save {
                let cents = |t: &str| parse_cents(t).unwrap_or(0);
                let (row, date, note) = (ed.row, ed.date, ed.note.trim().to_string());
                let (spent, cc, amount) = (cents(&ed.spent), cents(&ed.cc), cents(&ed.amount));
                let result = match row {
                    RowRef::Purchase(id) => self.db().update_purchase(id, date, spent, cc, &note),
                    RowRef::Redemption(id) => self.db().update_redemption(id, date, amount, &note),
                };
                self.apply(result);
            }
            if save || close {
                self.editing = None;
            }
        }

        if let Some((casino_id, text)) = &mut self.pasting {
            let casino_id = *casino_id;
            let mut close = false;
            let mut import = false;
            let rows = parse_spreadsheet(text);
            dialog(ctx, "Paste from spreadsheet", |ui| {
                ui.label("Copy rows from Excel / Google Sheets and paste them below.");
                ui.label(dim(
                    "Columns: DATE, SPENT, CC RECEIVED, WITHDRAWAL (header row is optional)",
                ));
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        theme::input(
                            ui,
                            egui::TextEdit::multiline(text)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(12)
                                .hint_text("3/13/2026\t90\t100\t\n3/14/2026\t\t\t500"),
                            476.0,
                        );
                    });
                let s = Stats {
                    spent: rows.iter().map(|r| r.spent).sum(),
                    cc_received: rows.iter().map(|r| r.cc_received).sum(),
                    withdrawn: rows.iter().map(|r| r.withdrawal).sum(),
                };
                ui.label(format!(
                    "{} rows found — spent {}, {} CC, withdrawn {}",
                    rows.len(),
                    money(s.spent),
                    coins(s.cc_received),
                    money(s.withdrawn)
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    import = theme::button_enabled(ui, !rows.is_empty(), "Import").clicked();
                    close = theme::button(ui, "Cancel").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
            if import {
                let result = self.db().import_rows(casino_id, &rows);
                self.apply(result);
            }
            if import || close {
                self.pasting = None;
            }
        }

        if let Some((casino_id, name)) = &mut self.renaming {
            let casino_id = *casino_id;
            let mut close = false;
            let mut save = false;
            dialog(ctx, "Rename casino", |ui| {
                let r = theme::input(ui, egui::TextEdit::singleline(name), 280.0);
                save |= submitted(ui, &r);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save |= theme::button(ui, "Save").clicked();
                    close = theme::button(ui, "Cancel").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
            let new_name = name.trim().to_string();
            if save && !new_name.is_empty() {
                let result = self.db().rename_casino(casino_id, &new_name);
                self.apply(result);
            }
            if save || close {
                self.renaming = None;
            }
        }

        if let Some(id) = self.confirm_delete {
            let name = self
                .data
                .casino(id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let mut close = false;
            let mut delete = false;
            dialog(ctx, "Delete casino?", |ui| {
                ui.label(format!("Delete {name} and all of its entries?"));
                ui.label(
                    dim("They're hidden from the app but kept in the project's database.").small(),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    delete = theme::button(ui, RichText::new("Delete").color(LOSS)).clicked();
                    close = theme::button(ui, "Cancel").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
            if delete {
                let result = self.db().delete_casino(id);
                if self.apply(result).is_some() {
                    self.view = View::Overview;
                }
            }
            if delete || close {
                self.confirm_delete = None;
            }
        }
    }
}

/// A purchase or a redemption, for showing both in one table.
enum Row<'a> {
    Purchase(&'a Purchase),
    Redemption(&'a Redemption),
}

impl Row<'_> {
    fn date(&self) -> NaiveDate {
        match self {
            Row::Purchase(p) => p.date,
            Row::Redemption(r) => r.date,
        }
    }

    fn note(&self) -> &str {
        match self {
            Row::Purchase(p) => &p.note,
            Row::Redemption(r) => &r.note,
        }
    }

    fn row_ref(&self) -> RowRef {
        match self {
            Row::Purchase(p) => RowRef::Purchase(p.id),
            Row::Redemption(r) => RowRef::Redemption(r.id),
        }
    }

    /// Date, then redemptions after purchases on the same day, then entry order.
    fn sort_key(&self) -> (NaiveDate, u8, i64) {
        match self {
            Row::Purchase(p) => (p.date, 0, p.id),
            Row::Redemption(r) => (r.date, 1, r.id),
        }
    }

    fn edit_state(&self) -> EditState {
        match self {
            Row::Purchase(p) => EditState {
                row: RowRef::Purchase(p.id),
                date: p.date,
                spent: edit_text(p.spent),
                cc: edit_text(p.cc_received),
                amount: String::new(),
                note: p.note.clone(),
            },
            Row::Redemption(r) => EditState {
                row: RowRef::Redemption(r.id),
                date: r.date,
                spent: String::new(),
                cc: String::new(),
                amount: edit_text(r.amount),
                note: r.note.clone(),
            },
        }
    }
}

/// One casino in the "Add casino" dropdown: logo, name, signup bonus and redeem speed, tier.
fn picker_row(ui: &mut egui::Ui, g: &GrailCasino, added: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
    let p = ui.painter();
    // Casinos you can still add get a tinted background; ones already in your list stay white.
    let bg = match (added, resp.hovered()) {
        (false, false) => theme::CONTENT_BG,
        (false, true) => theme::CONTENT_BG_HOVER,
        (true, true) => theme::STRIPE,
        (true, false) => theme::BG,
    };
    // Shrunk a little vertically so neighbouring tinted rows stay visually separate.
    p.rect_filled(rect.shrink2(Vec2::new(0.0, 1.5)), 0.0, bg);
    let logo = egui::Rect::from_min_size(
        rect.left_center() + Vec2::new(4.0, -17.0),
        Vec2::splat(34.0),
    );
    theme::logo_at(ui, &g.logo, logo);

    let badge = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 18.0, rect.center().y),
        Vec2::splat(24.0),
    );
    // Text stops short of the tier badge instead of running under it.
    let mut text_area = rect;
    text_area.set_right(badge.left() - 8.0);
    let p = &p.with_clip_rect(text_area);

    let x = logo.right() + 10.0;
    let name_color = if added { DIM } else { PRIMARY };
    let name_rect = p.text(
        egui::pos2(x, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        &g.name,
        FontId::new(15.0, theme::bold()),
        name_color,
    );
    let speed = g.redeem_speed.split(' ').next().unwrap_or_default();
    let detail: Vec<String> = [
        (!g.signup_bonus.is_empty()).then(|| format!("{} signup", g.signup_bonus)),
        (!speed.is_empty()).then(|| format!("{speed} redeems")),
        (!g.cash_min.is_empty()).then(|| format!("{} min cash", g.cash_min)),
    ]
    .into_iter()
    .flatten()
    .collect();
    p.text(
        egui::pos2(x, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        detail.join("  ·  "),
        FontId::proportional(12.5),
        DIM,
    );

    if !g.tier.is_empty() {
        theme::paint_tier(ui.painter(), badge, &g.tier);
    }
    // ADDED / NEW tag sits right after the name, outlined like the casino page's NEW tag.
    let tag = if added {
        Some(("ADDED", DIM))
    } else if g.is_new {
        Some(("NEW", GAIN))
    } else {
        None
    };
    if let Some((label, color)) = tag {
        let galley = p.layout_no_wrap(label.into(), FontId::new(11.0, theme::bold()), color);
        let tag_rect = egui::Rect::from_min_size(
            egui::pos2(
                name_rect.right() + 8.0,
                name_rect.center().y - galley.size().y / 2.0 - 2.0,
            ),
            galley.size() + Vec2::new(10.0, 4.0),
        );
        p.rect_stroke(tag_rect, 0.0, Stroke::new(theme::BORDER, color));
        p.galley(tag_rect.min + Vec2::new(5.0, 2.0), galley, color);
    }

    let tip = if added {
        "Already in your list. Click to open it.".to_string()
    } else if g.welcome_bonus.is_empty() {
        g.name.clone()
    } else {
        g.welcome_bonus.clone()
    };
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// SweepsGrail details for a casino, shown under its page header: a header with the review
/// button, then equal-width tiles in rows (redemptions, bonuses, company), then the welcome offer.
fn grail_info(ui: &mut egui::Ui, g: &GrailCasino) {
    // Flush grid: no padding inside the box, tiles edge to edge with thin lines between.
    theme::boxed_flush(ui, |ui| {
        // "Normal (next day to 3 days)" -> "Normal" + "next day to 3 days"
        let (speed, speed_detail) = match g.redeem_speed.split_once(" (") {
            Some((s, d)) => (s, d.trim_end_matches(')')),
            None => (g.redeem_speed.as_str(), ""),
        };
        let vip = if g.vip { "Yes" } else { "No" };
        // (label, value, hover detail)
        let mut rows: Vec<Vec<(String, String, String)>> = vec![
            vec![
                tile("REDEEM SPEED", speed, speed_detail),
                tile("MIN CASH", &g.cash_min, "Minimum cash redemption"),
                tile("MIN GIFT CARD", &g.gift_min, "Minimum gift card redemption"),
                tile("MIN CRYPTO", &g.crypto_min, "Minimum crypto redemption"),
                tile("SIGNUP BONUS", &g.signup_bonus, "Free on signup"),
            ],
            vec![
                tile("DAILY FREEBIE", &g.daily_freebie, "Daily login bonus value"),
                tile("PROMOS", &g.promo_value, "Promotion value"),
                tile("VIP", vip, &g.vip_note),
                tile("PARENT", &g.parent, "Parent company"),
                tile("LOCATED", &g.located, ""),
            ],
        ];
        // Welcome offer parts ("Signup SC: 2.5 free SC on signup") become a row of tiles.
        if !g.welcome_bonus.is_empty() && g.welcome_bonus != g.signup_bonus {
            let parts: Vec<_> = g
                .welcome_bonus
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|part| {
                    let (key, value) = part.split_once(": ").unwrap_or(("Welcome offer", part));
                    tile(&key.to_uppercase(), value, value)
                })
                .collect();
            if !parts.is_empty() {
                rows.push(parts);
            }
        }
        for (r, row) in rows.iter().enumerate() {
            ui.columns(row.len(), |cols| {
                for (c, (col, (label, value, detail))) in
                    cols.iter_mut().zip(row.iter()).enumerate()
                {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                    theme::info_tile(col, label, value, detail, c == 0, r == 0);
                }
            });
        }
    });
    ui.add_space(theme::SHADOW);
}

fn tile(label: &str, value: &str, detail: &str) -> (String, String, String) {
    (label.to_string(), value.to_string(), detail.to_string())
}

/// "just now", "5 min ago", "3 h ago", "2 days ago"
fn ago(unix_secs: i64) -> String {
    let secs = (chrono::Utc::now().timestamp() - unix_secs).max(0);
    match secs {
        0..=89 => "just now".into(),
        90..=5399 => format!("{} min ago", secs / 60),
        5400..=129_599 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn dialog(ctx: &egui::Context, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, add_contents);
}

/// Simple line chart of cumulative net profit (withdrawn - spent) by day.
/// Line chart of cumulative net profit (redeemed - spent) by day. Values are cents.
fn profit_chart(ui: &mut egui::Ui, by_day: BTreeMap<NaiveDate, Cents>) {
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width() - theme::SHADOW, 200.0),
        Sense::hover(),
    );
    let rect = resp.rect;
    painter.rect_filled(rect.translate(Vec2::splat(theme::SHADOW)), 0.0, PRIMARY);
    painter.rect_filled(rect, 0.0, theme::BG);
    painter.rect_stroke(
        rect.shrink(theme::BORDER / 2.0),
        0.0,
        Stroke::new(theme::BORDER, PRIMARY),
    );

    if by_day.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Add entries on at least two different days to see a chart",
            FontId::proportional(14.0),
            DIM,
        );
        return;
    }

    let mut total: Cents = 0;
    let points: Vec<(NaiveDate, Cents)> = by_day
        .into_iter()
        .map(|(d, v)| {
            total += v;
            (d, total)
        })
        .collect();
    let first = points[0].0;
    let span = (points.last().unwrap().0 - first).num_days().max(1) as f32;
    let min = points.iter().map(|p| p.1).min().unwrap_or(0).min(0) as f32;
    let max = points.iter().map(|p| p.1).max().unwrap_or(0).max(0) as f32;
    let range = (max - min).max(1.0);

    let plot = rect.shrink2(Vec2::new(64.0, 22.0));
    let to_screen = |d: NaiveDate, v: Cents| {
        let x = plot.left() + plot.width() * (d - first).num_days() as f32 / span;
        let y = plot.bottom() - plot.height() * (v as f32 - min) / range;
        egui::pos2(x, y)
    };

    // Zero line and axis labels
    let zero_y = to_screen(first, 0).y;
    painter.line_segment(
        [
            egui::pos2(plot.left(), zero_y),
            egui::pos2(plot.right(), zero_y),
        ],
        Stroke::new(theme::BORDER, ACCENT),
    );
    let small = FontId::proportional(11.5);
    for v in [max, 0.0, min] {
        let y = plot.bottom() - plot.height() * (v - min) / range;
        painter.text(
            egui::pos2(rect.left() + 8.0, y),
            Align2::LEFT_CENTER,
            money(v as Cents),
            small.clone(),
            DIM,
        );
    }
    let last = points.last().unwrap().0;
    painter.text(
        egui::pos2(plot.left(), rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        first.format("%-m/%-d").to_string(),
        small.clone(),
        DIM,
    );
    painter.text(
        egui::pos2(plot.right(), rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        last.format("%-m/%-d").to_string(),
        small.clone(),
        DIM,
    );

    let screen: Vec<egui::Pos2> = points.iter().map(|&(d, v)| to_screen(d, v)).collect();
    painter.add(egui::Shape::line(
        screen.clone(),
        Stroke::new(2.5_f32, PRIMARY),
    ));

    // Hover: highlight the nearest day
    if let Some(pos) = resp.hover_pos() {
        if let Some((i, p)) = screen
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.x - pos.x).abs().total_cmp(&(b.1.x - pos.x).abs()))
        {
            painter.rect_filled(
                egui::Rect::from_center_size(*p, Vec2::splat(8.0)),
                0.0,
                PRIMARY,
            );
            let (d, v) = points[i];
            let label = format!("{}  {}", d.format("%-m/%-d/%Y"), signed_money(v));
            let align = if p.x > rect.center().x {
                Align2::RIGHT_BOTTOM
            } else {
                Align2::LEFT_BOTTOM
            };
            painter.text(
                *p + Vec2::new(0.0, -10.0),
                align,
                label,
                FontId::proportional(13.0),
                profit_color(v),
            );
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_grail();
        // Title bar first so it spans the full window width above the sidebar.
        let project = self.project.as_deref().map(project_name);
        titlebar::show(ctx, project.as_deref());
        titlebar::resize_edges(ctx);

        let shortcut = |key: Key, shift: bool| {
            let mods = if shift {
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::COMMAND
            };
            ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(mods, key)))
        };
        if shortcut(Key::S, true) && self.project.is_some() {
            self.action = Some(ProjectAction::SaveAs);
        } else if shortcut(Key::N, false) {
            self.action = Some(ProjectAction::New);
        } else if shortcut(Key::O, false) {
            self.action = Some(ProjectAction::Open);
        }

        if self.status.is_some() {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.status.as_deref().unwrap_or_default()).color(LOSS));
                    if theme::button(ui, "✕").clicked() {
                        self.status = None;
                    }
                });
            });
        }

        if self.project.is_none() {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::central_panel(&ctx.style())
                        .fill(theme::CONTENT_BG)
                        .inner_margin(egui::Margin::same(20.0)),
                )
                .show(ctx, |ui| self.welcome(ui));
            if let Some(action) = self.action.take() {
                self.run_action(ctx, action);
            }
            return;
        }

        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(240.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0)),
            )
            .show(ctx, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(theme::CONTENT_BG)
                    .inner_margin(egui::Margin::same(20.0)),
            )
            .show(ctx, |ui| match self.view {
                View::Overview => self.overview(ui),
                View::Casino(id) => self.casino_view(ui, id),
            });

        self.casino_picker(ctx);
        self.dialogs(ctx);

        if let Some(action) = self.action.take() {
            self.run_action(ctx, action);
        }
    }
}
