//! Pop-up dialogs: edit entry, paste rows, rename and delete casino.

use super::*;

impl App {
    /// Modal-ish dialogs: edit entry, paste rows, rename and delete casino.
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(ed) = &mut self.editing {
            let mut close = false;
            let mut save = false;
            let title = match ed.row {
                EntryRef::Purchase(_) => "Edit purchase",
                EntryRef::Redemption(_) => "Edit withdrawal",
                EntryRef::FreeCoins(_) => "Edit free SC",
            };
            let ok = |t: &str| t.trim().is_empty() || parse_cents(t).is_some_and(|v| v >= 0);
            let cents = |t: &str| parse_cents(t).unwrap_or(0);
            dialog(ctx, title, |ui| {
                egui::Grid::new("edit_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        let date_label = match ed.row {
                            EntryRef::Redemption(_) => "REQUESTED",
                            _ => "DATE",
                        };
                        ui.label(dim(date_label));
                        ui.add(date_picker(&mut ed.date, "edit_date"));
                        ui.end_row();
                        match ed.row {
                            EntryRef::Purchase(_) => {
                                ui.label(dim("SPENT $"));
                                amount_field(ui, &mut ed.spent, "0", 160.0);
                                ui.end_row();
                                ui.label(dim("CC RECEIVED"));
                                amount_field(ui, &mut ed.cc, "0", 160.0);
                                ui.end_row();
                            }
                            EntryRef::Redemption(_) => {
                                ui.label(dim("AMOUNT $"));
                                amount_field(ui, &mut ed.amount, "0", 160.0);
                                ui.end_row();
                                ui.label(dim("METHOD"));
                                choice(
                                    ui,
                                    "edit_method",
                                    &mut ed.method,
                                    &Method::ALL,
                                    Method::label,
                                    160.0,
                                );
                                ui.end_row();
                                ui.label(dim("FEE $"));
                                amount_field(ui, &mut ed.fee, "0", 160.0);
                                ui.end_row();
                                ui.label(dim("STATUS"));
                                choice(
                                    ui,
                                    "edit_status",
                                    &mut ed.received,
                                    &[true, false],
                                    status_label,
                                    160.0,
                                );
                                ui.end_row();
                                if ed.received {
                                    ui.label(dim("RECEIVED"));
                                    ui.add(date_picker(&mut ed.received_date, "edit_received"));
                                    ui.end_row();
                                }
                            }
                            EntryRef::FreeCoins(_) => {
                                ui.label(dim("SC"));
                                amount_field(ui, &mut ed.sc, "0", 160.0);
                                ui.end_row();
                                ui.label(dim("SOURCE"));
                                choice(
                                    ui,
                                    "edit_source",
                                    &mut ed.source,
                                    &Source::ALL,
                                    Source::label,
                                    160.0,
                                );
                                ui.end_row();
                            }
                        }
                        ui.label(dim("NOTE"));
                        theme::input(ui, egui::TextEdit::singleline(&mut ed.note), 220.0);
                        ui.end_row();
                    });
                let valid = match ed.row {
                    EntryRef::Purchase(_) => {
                        ok(&ed.spent) && ok(&ed.cc) && (cents(&ed.spent) > 0 || cents(&ed.cc) > 0)
                    }
                    EntryRef::Redemption(_) => cents(&ed.amount) > 0 && ok(&ed.fee),
                    EntryRef::FreeCoins(_) => cents(&ed.sc) > 0,
                };
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = theme::button_enabled(ui, valid, "Save").clicked();
                    close = theme::button(ui, "Cancel").clicked()
                        || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
            if save {
                let (date, note) = (ed.date, ed.note.trim().to_string());
                let result = match ed.row {
                    EntryRef::Purchase(id) => {
                        let (spent, cc) = (cents(&ed.spent), cents(&ed.cc));
                        self.db().update_purchase(id, date, spent, cc, &note)
                    }
                    EntryRef::Redemption(id) => {
                        let r = store::RedemptionInput {
                            date,
                            amount: cents(&ed.amount),
                            method: ed.method,
                            fee: cents(&ed.fee),
                            received: ed.received.then_some(ed.received_date),
                            note,
                        };
                        self.db().update_redemption(id, &r)
                    }
                    EntryRef::FreeCoins(id) => {
                        let (sc, source) = (cents(&ed.sc), ed.source);
                        self.db().update_free_coins(id, date, sc, source, &note)
                    }
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
                    ..Stats::default()
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
                ui.label(dim("You can restore it from Recently deleted in the sidebar.").small());
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
                    self.deleted_notice(Undo::Casino(id), format!("{name} and its entries"));
                }
            }
            if delete || close {
                self.confirm_delete = None;
            }
        }
    }
}
