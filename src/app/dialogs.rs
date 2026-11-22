//! Pop-up dialogs: edit entry, paste rows, rename and delete casino.

use super::*;

impl App {
    /// Modal-ish dialogs: edit entry, paste rows, rename and delete casino.
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
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
