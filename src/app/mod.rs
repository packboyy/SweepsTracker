//! The app window: shared state and helpers, and the per-frame update. Each screen lives
//! in its own submodule as more `impl App` methods.

mod casino;
mod dialogs;
mod overview;
mod picker;
mod project;
mod sidebar;

use crate::grail::{self, GrailCasino, GrailDb};
use crate::model::*;
use crate::store::{self, Store};
use crate::theme::{self, ACCENT, DIM, GAIN, LOSS, PRIMARY};
use crate::titlebar;
use chrono::{Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Layout, RichText, Sense, Stroke, Vec2,
};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

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

pub struct App {
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
    pub fn new(ctx: &egui::Context) -> Self {
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

    /// Clears per-project UI state when switching projects.
    fn reset_view(&mut self) {
        self.view = View::Overview;
        self.picker.open = false;
        self.editing = None;
        self.pasting = None;
        self.renaming = None;
        self.confirm_delete = None;
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
