//! Native macOS menu bar (Sweeps Tracker / File / Edit / View / Window / Help), built with
//! `muda`. Clicks arrive as `Command`s that the app runs like any other action. Items that
//! don't apply right now (no project open, nothing to undo...) are greyed out.
//!
//! Cut/Copy/Paste aren't in the Edit menu on purpose: their ⌘C/⌘V shortcuts would be taken
//! by the menu instead of reaching egui's text fields.

use super::project::REVEAL_LABEL;
use super::*;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::mpsc::{channel, Receiver};

pub(super) struct NativeMenu {
    /// Kept alive for as long as the app runs.
    _menu: Menu,
    clicks: Receiver<MenuId>,
    commands: Vec<(MenuId, Command)>,
    recent: Submenu,
    recent_items: Vec<(MenuId, PathBuf)>,
    recent_shown: Option<Vec<PathBuf>>,
    needs_project: Vec<MenuItem>,
    undo: MenuItem,
    paste_rows: MenuItem,
    /// (has project, can undo, on a casino page) last applied to the items.
    state: Option<(bool, bool, bool)>,
}

fn key(mods: Modifiers, code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Some(mods), code))
}

impl NativeMenu {
    pub(super) fn new(ctx: &egui::Context) -> Self {
        // Menu clicks happen outside egui's input, so wake the app up to handle them.
        let (tx, clicks) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = tx.send(event.id);
            ctx.request_repaint();
        }));

        let mut commands = Vec::new();
        let mut item = |text: &str, accelerator: Option<Accelerator>, command: Command| {
            let item = MenuItem::new(text, true, accelerator);
            commands.push((item.id().clone(), command));
            item
        };
        let cmd = Modifiers::SUPER;

        let new_project = item("New Project…", key(cmd, Code::KeyN), Command::New);
        let open = item("Open Folder…", key(cmd, Code::KeyO), Command::Open);
        let save_as = item(
            "Save As…",
            key(cmd | Modifiers::SHIFT, Code::KeyS),
            Command::SaveAs,
        );
        let export = item("Export CSV…", key(cmd, Code::KeyE), Command::ExportCsv);
        let reveal = item(REVEAL_LABEL, None, Command::Reveal);
        let close = item("Close Project", None, Command::Close);
        let undo = item("Undo Delete", None, Command::UndoDelete);
        let add_casino = item("Add Casino…", None, Command::AddCasino);
        let paste_rows = item("Paste Rows…", None, Command::PasteRows);
        let overview = item(
            "Overview",
            key(cmd, Code::Digit1),
            Command::Go(View::Overview),
        );
        let taxes = item("Taxes", key(cmd, Code::Digit2), Command::Go(View::Taxes));
        let trash = item(
            "Recently Deleted",
            key(cmd, Code::Digit3),
            Command::Go(View::Trash),
        );
        let website = item("SweepsGrail Website", None, Command::SweepsGrail);

        let separator = PredefinedMenuItem::separator;
        let about = PredefinedMenuItem::about(
            Some("About Sweeps Tracker"),
            Some(AboutMetadata {
                name: Some("Sweeps Tracker".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                ..Default::default()
            }),
        );
        let app_menu = Submenu::with_items(
            "Sweeps Tracker",
            true,
            &[
                &about,
                &separator(),
                &PredefinedMenuItem::services(None),
                &separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )
        .expect("app menu");
        let recent = Submenu::new("Open Recent", true);
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &new_project,
                &open,
                &recent,
                &separator(),
                &save_as,
                &export,
                &separator(),
                &reveal,
                &close,
            ],
        )
        .expect("file menu");
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[&undo, &separator(), &add_casino, &paste_rows],
        )
        .expect("edit menu");
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &overview,
                &taxes,
                &trash,
                &separator(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )
        .expect("view menu");
        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(Some("Zoom")),
                &separator(),
                &PredefinedMenuItem::bring_all_to_front(None),
            ],
        )
        .expect("window menu");
        let help = Submenu::with_items("Help", true, &[&website]).expect("help menu");

        let menu =
            Menu::with_items(&[&app_menu, &file, &edit, &view, &window, &help]).expect("menu bar");
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();

        NativeMenu {
            _menu: menu,
            clicks,
            commands,
            recent,
            recent_items: Vec::new(),
            recent_shown: None,
            needs_project: vec![
                save_as, export, reveal, close, add_casino, overview, taxes, trash,
            ],
            undo,
            paste_rows,
            state: None,
        }
    }

    /// Commands clicked (or triggered by shortcut) since the last frame.
    pub(super) fn poll(&self) -> Vec<Command> {
        self.clicks
            .try_iter()
            .filter_map(|id| {
                self.commands
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, c)| c.clone())
                    .or_else(|| {
                        self.recent_items
                            .iter()
                            .find(|(i, _)| *i == id)
                            .map(|(_, dir)| Command::OpenRecent(dir.clone()))
                    })
            })
            .collect()
    }

    /// Enables/disables items and rebuilds Open Recent, only when something changed.
    pub(super) fn sync(
        &mut self,
        has_project: bool,
        can_undo: bool,
        on_casino: bool,
        recent: &[PathBuf],
    ) {
        let state = (has_project, can_undo, on_casino);
        if self.state != Some(state) {
            for item in &self.needs_project {
                item.set_enabled(has_project);
            }
            self.undo.set_enabled(can_undo);
            self.paste_rows.set_enabled(on_casino);
            self.state = Some(state);
        }
        if self.recent_shown.as_deref() != Some(recent) {
            while self.recent.remove_at(0).is_some() {}
            self.recent_items.clear();
            if recent.is_empty() {
                let _ = self
                    .recent
                    .append(&MenuItem::new("No Recent Projects", false, None));
            }
            for dir in recent {
                let item = MenuItem::new(project_name(dir), true, None);
                self.recent_items.push((item.id().clone(), dir.clone()));
                let _ = self.recent.append(&item);
            }
            self.recent_shown = Some(recent.to_vec());
        }
    }
}
