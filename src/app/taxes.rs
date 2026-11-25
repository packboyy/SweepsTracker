//! Taxes page: a tax year's redemptions, purchases and fees, the estimated deductible losses,
//! pending redemptions, a per-casino breakdown, and CSV export.

use super::*;
use crate::tax::{self, TaxSummary};

impl App {
    pub(super) fn taxes(&mut self, ui: &mut egui::Ui) {
        let years = self.cache.years.clone();
        if !years.contains(&self.tax_year) {
            self.tax_year = years[0];
        }
        let mut export: Option<Option<i32>> = None;
        let year = self.tax_year;
        page_header(ui, "Taxes", |ui| {
            ui.add_space(theme::SHADOW);
            if theme::button(ui, "Export all years")
                .on_hover_text("Every entry as a CSV file")
                .clicked()
            {
                export = Some(None);
            }
            let export_year = theme::button(ui, format!("Export {year}")).on_hover_text(format!(
                "{year}'s entries as a CSV file for your tax preparer"
            ));
            if export_year.clicked() {
                export = Some(Some(year));
            }
            // Same height as the Export buttons beside it.
            let size = Vec2::new(110.0, export_year.rect.height());
            choice_sized(
                ui,
                "tax_year",
                &mut self.tax_year,
                &years,
                |y| y.to_string(),
                size,
            );
            ui.label(dim_text("YEAR").small());
        });

        // Computed once per year shown (and again after data changes), not every frame.
        if self.cache.tax.as_ref().map(|s| s.year) != Some(self.tax_year) {
            self.cache.tax = Some(TaxSummary::for_year(&self.data, self.tax_year));
        }
        let Some(s) = &self.cache.tax else { return };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(dim_text(
                    "Estimates for you or your tax preparer, not tax advice. Redemptions count in \
                     the year the money was received; purchases in the year they were made.",
                ));
                ui.add_space(10.0);
                summary(ui, &s);
                ui.add_space(14.0);
                filing(ui, &s);
                ui.add_space(20.0);

                theme::section(ui, "Pending redemptions");
                ui.add_space(4.0);
                let rows = s
                    .pending
                    .iter()
                    .map(|p| {
                        vec![
                            (p.casino.clone(), primary()),
                            (p.date.format("%-m/%-d/%Y").to_string(), primary()),
                            (money(p.amount), primary()),
                            (p.method.label().to_string(), dim()),
                        ]
                    })
                    .collect();
                data_table(
                    ui,
                    &["CASINO", "REQUESTED", "AMOUNT", "METHOD"],
                    rows,
                    "Nothing pending. Every redemption has been received.",
                );
                ui.add_space(20.0);

                theme::section(ui, &format!("By casino ({})", s.year));
                ui.add_space(4.0);
                let rows = s
                    .casinos
                    .iter()
                    .map(|c| {
                        vec![
                            (c.name.clone(), primary()),
                            (money(c.purchases), primary()),
                            (money(c.redeemed), primary()),
                            (money(c.fees), primary()),
                            (signed_money(c.net()), profit_color(c.net())),
                        ]
                    })
                    .collect();
                data_table(
                    ui,
                    &["CASINO", "PURCHASES", "REDEEMED", "FEES", "NET"],
                    rows,
                    "No purchases or received redemptions this year.",
                );
                ui.add_space(theme::SHADOW + 4.0);
            });

        if let Some(year) = export {
            self.export_csv(year);
        }
    }

    /// Asks where to save, then writes the CSV (one year, or everything).
    pub(super) fn export_csv(&mut self, year: Option<i32>) {
        let name = match year {
            Some(y) => format!("sweeps-{y}.csv"),
            None => "sweeps-all.csv".to_string(),
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export entries as CSV")
            .set_file_name(&name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, tax::csv(&self.data, year)) {
            Ok(()) => {
                self.status = None;
                self.undo = None;
                self.notice = Some(format!("Exported to {}", path.display()));
            }
            Err(e) => self.status = Some(format!("Couldn't export: {e}")),
        }
    }
}

/// Two rows of totals: income/fees/purchases/net, then payout methods and free SC.
fn summary(ui: &mut egui::Ui, s: &TaxSummary) {
    let rows: [[(&str, String, Color32); 4]; 2] = [
        [
            ("REDEMPTIONS RECEIVED", money(s.redeemed), primary()),
            ("FEES", money(s.fees), primary()),
            ("PURCHASES", money(s.purchases), primary()),
            ("NET", signed_money(s.net()), profit_color(s.net())),
        ],
        [
            ("BY BANK", money(s.by_method[0]), primary()),
            ("BY GIFT CARD", money(s.by_method[1]), primary()),
            ("BY CRYPTO", money(s.by_method[2]), primary()),
            ("FREE SC COLLECTED", coins(s.free_sc), primary()),
        ],
    ];
    theme::boxed_flush(ui, |ui| {
        for (r, row) in rows.into_iter().enumerate() {
            ui.columns(row.len(), |cols| {
                for (c, (col, (title, value, color))) in cols.iter_mut().zip(row).enumerate() {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                    theme::grid_cell(col, c == 0, r == 0, None, CELL_PAD, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(egui::Label::new(dim_text(title).small()).truncate());
                        let size = if r == 0 { 22.0 } else { 17.0 };
                        ui.label(
                            RichText::new(value)
                                .size(size)
                                .family(theme::bold())
                                .color(color),
                        );
                    });
                }
            });
        }
    });
    ui.add_space(theme::SHADOW);
}

/// The two ways to file side by side: what each adds to AGI (which decides credits like the
/// AOTC) in large type, then the amounts behind it as label / amount rows, then a note.
fn filing(ui: &mut egui::Ui, s: &TaxSummary) {
    let pct = s.loss_percent();
    let costs = |what: &str| {
        if pct < 100 {
            format!("{pct}% of {what}")
        } else {
            what.to_string()
        }
    };
    let casual = s.casual_agi();
    let pro = s.schedule_c_agi();
    let cells = [
        (
            "CASUAL PLAYER",
            casual,
            casual < pro,
            vec![
                ("Redemptions, all counted as income".to_string(), s.redeemed),
                (
                    format!("Losses ({}), only if you itemize", costs("purchases")),
                    s.deductible_losses(),
                ),
            ],
        ),
        (
            "PROFESSIONAL · SCHEDULE C",
            pro,
            pro < casual,
            vec![
                (
                    format!("Profit after {}", costs("purchases and fees")),
                    s.schedule_c_profit(),
                ),
                (
                    "Self-employment tax (half deducted)".to_string(),
                    s.self_employment_tax(),
                ),
            ],
        ),
    ];
    theme::boxed_flush(ui, |ui| {
        ui.columns(2, |cols| {
            for (c, (col, (title, agi, lower, rows))) in cols.iter_mut().zip(cells).enumerate() {
                col.spacing_mut().item_spacing = Vec2::ZERO;
                theme::grid_cell(col, c == 0, true, None, CELL_PAD, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
                    ui.horizontal(|ui| {
                        ui.label(dim_text(title).small());
                        if lower {
                            theme::tag(ui, "LOWER AGI", gain());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(money(agi)).size(26.0).family(theme::bold()));
                        ui.label(dim_text("added to AGI"));
                    });
                    ui.add_space(4.0);
                    // Label in a fixed-width column (wrapping if it must), amount in a
                    // right-aligned column after it.
                    let amount_width = 110.0;
                    for (label, amount) in rows {
                        let label_width = ui.available_width() - amount_width;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui(Vec2::new(label_width, 0.0), |ui| {
                                ui.set_width(label_width);
                                ui.add(egui::Label::new(dim_text(label).small()).wrap());
                            });
                            let size = Vec2::new(ui.available_width(), 0.0);
                            ui.allocate_ui_with_layout(
                                size,
                                Layout::right_to_left(Align::Min),
                                |ui| {
                                    ui.label(RichText::new(money(amount)).family(theme::bold()));
                                },
                            );
                        });
                    }
                });
            }
        });
        theme::grid_cell(ui, true, false, Some(theme::stripe()), CELL_PAD, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                RichText::new("A lower AGI can keep credits like the AOTC.").family(theme::bold()),
            );
            ui.label(dim_text(
                "If you play regularly to make a profit and keep records of it (like this app), \
                 you may qualify to file as a professional gambler on Schedule C and still take \
                 the standard deduction. Estimates only; ask your tax preparer which way is \
                 better for you.",
            ).small());
        });
    });
    ui.add_space(theme::SHADOW);
}
