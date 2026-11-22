//! "Recently deleted": soft-deleted casinos and entries, with Restore buttons.

use super::*;

/// What a Restore button restores.
enum Restore {
    Casino(i64),
    Entry(EntryRef),
}

impl App {
    pub(super) fn trash(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Recently deleted", |_| {});
        ui.label(dim(
            "Deleted casinos and entries are kept in the project's database. Restore them here.",
        ));
        ui.add_space(12.0);

        let mut restore = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::section(ui, "Casinos");
                ui.add_space(4.0);
                let rows: Vec<Vec<String>> = self
                    .deleted
                    .casinos
                    .iter()
                    .map(|c| {
                        vec![
                            c.name.clone(),
                            c.entries.to_string(),
                            local_time(&c.deleted_at),
                        ]
                    })
                    .collect();
                let headers = ["CASINO", "ENTRIES", "DELETED"];
                if let Some(i) = restore_table(ui, &headers, rows, "No deleted casinos.") {
                    restore = Some(Restore::Casino(self.deleted.casinos[i].id));
                }
                ui.add_space(20.0);

                theme::section(ui, "Entries");
                ui.add_space(4.0);
                let rows: Vec<Vec<String>> = self
                    .deleted
                    .entries
                    .iter()
                    .map(|e| {
                        let kind = match e.entry {
                            EntryRef::Purchase(_) => "Purchase",
                            EntryRef::Redemption(_) => "Withdrawal",
                            EntryRef::FreeCoins(_) => "Free SC",
                        };
                        vec![
                            e.casino.clone(),
                            kind.to_string(),
                            e.date.format("%-m/%-d/%Y").to_string(),
                            match e.entry {
                                EntryRef::FreeCoins(_) => format!("{} SC", coins(e.amount)),
                                _ => money(e.amount),
                            },
                            local_time(&e.deleted_at),
                        ]
                    })
                    .collect();
                let headers = ["CASINO", "TYPE", "DATE", "AMOUNT", "DELETED"];
                if let Some(i) = restore_table(ui, &headers, rows, "No deleted entries.") {
                    restore = Some(Restore::Entry(self.deleted.entries[i].entry));
                }
                ui.add_space(theme::SHADOW + 4.0);
            });

        match restore {
            Some(Restore::Casino(id)) => {
                let result = self.db().restore_casino(id);
                if self.apply(result).is_some() {
                    self.view = View::Casino(id);
                }
            }
            Some(Restore::Entry(entry)) => {
                let result = self.db().restore_entry(entry);
                self.apply(result);
            }
            None => {}
        }
    }
}

/// Flush grid table with a Restore button at the end of each row. Returns the index of the
/// row whose button was clicked.
fn restore_table(
    ui: &mut egui::Ui,
    headers: &[&str],
    rows: Vec<Vec<String>>,
    empty: &str,
) -> Option<usize> {
    let pad = Vec2::new(10.0, 6.0);
    let mut clicked = None;
    theme::boxed_flush(ui, |ui| {
        ui.columns(headers.len() + 1, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().item_spacing = Vec2::ZERO;
            }
            for (c, h) in headers.iter().chain([&""]).enumerate() {
                theme::grid_cell(&mut cols[c], c == 0, true, None, pad, |ui| {
                    ui.add(egui::Label::new(dim(*h).small()).truncate());
                });
            }
        });
        if rows.is_empty() {
            theme::grid_cell(ui, true, false, None, Vec2::new(10.0, 12.0), |ui| {
                ui.label(dim(empty));
            });
        }
        for (r, row) in rows.into_iter().enumerate() {
            let fill = (r % 2 == 1).then_some(theme::STRIPE);
            ui.columns(headers.len() + 1, |cols| {
                for col in cols.iter_mut() {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                }
                let last = cols.len() - 1;
                for (c, text) in row.into_iter().enumerate() {
                    theme::grid_cell(&mut cols[c], c == 0, false, fill, pad, |ui| {
                        // Same height as the Restore button in the last cell.
                        let size = [ui.available_width(), ui.spacing().interact_size.y];
                        ui.add_sized(size, egui::Label::new(text).truncate());
                    });
                }
                theme::grid_cell(&mut cols[last], false, false, fill, pad, |ui| {
                    if theme::button(ui, "Restore").clicked() {
                        clicked = Some(r);
                    }
                });
            });
        }
    });
    ui.add_space(theme::SHADOW);
    clicked
}
