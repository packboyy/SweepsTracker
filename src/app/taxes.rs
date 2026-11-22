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
            if theme::button(ui, format!("Export {year}"))
                .on_hover_text(format!(
                    "{year}'s entries as a CSV file for your tax preparer"
                ))
                .clicked()
            {
                export = Some(Some(year));
            }
            choice(
                ui,
                "tax_year",
                &mut self.tax_year,
                &years,
                |y| y.to_string(),
                110.0,
            );
            ui.label(dim("YEAR").small());
        });

        // Computed once per year shown (and again after data changes), not every frame.
        if self.cache.tax.as_ref().map(|s| s.year) != Some(self.tax_year) {
            self.cache.tax = Some(TaxSummary::for_year(&self.data, self.tax_year));
        }
        let Some(s) = &self.cache.tax else { return };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(dim(
                    "Estimates for you or your tax preparer, not tax advice. Redemptions count in \
                     the year the money was received; purchases in the year they were made.",
                ));
                ui.add_space(10.0);
                summary(ui, &s);
                ui.add_space(14.0);
                deductible(ui, &s);
                ui.add_space(20.0);

                theme::section(ui, "Pending redemptions");
                ui.add_space(4.0);
                let rows = s
                    .pending
                    .iter()
                    .map(|p| {
                        vec![
                            (p.casino.clone(), PRIMARY),
                            (p.date.format("%-m/%-d/%Y").to_string(), PRIMARY),
                            (money(p.amount), PRIMARY),
                            (p.method.label().to_string(), DIM),
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
                            (c.name.clone(), PRIMARY),
                            (money(c.purchases), PRIMARY),
                            (money(c.redeemed), PRIMARY),
                            (money(c.fees), PRIMARY),
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
    fn export_csv(&mut self, year: Option<i32>) {
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
            ("REDEMPTIONS RECEIVED", money(s.redeemed), PRIMARY),
            ("FEES", money(s.fees), PRIMARY),
            ("PURCHASES", money(s.purchases), PRIMARY),
            ("NET", signed_money(s.net()), profit_color(s.net())),
        ],
        [
            ("BY BANK", money(s.by_method[0]), PRIMARY),
            ("BY GIFT CARD", money(s.by_method[1]), PRIMARY),
            ("BY CRYPTO", money(s.by_method[2]), PRIMARY),
            ("FREE SC COLLECTED", coins(s.free_sc), PRIMARY),
        ],
    ];
    theme::boxed_flush(ui, |ui| {
        for (r, row) in rows.into_iter().enumerate() {
            ui.columns(row.len(), |cols| {
                for (c, (col, (title, value, color))) in cols.iter_mut().zip(row).enumerate() {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                    theme::grid_cell(col, c == 0, r == 0, None, CELL_PAD, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(egui::Label::new(dim(title).small()).truncate());
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

/// The estimated deductible losses, with the rule spelled out.
fn deductible(ui: &mut egui::Ui, s: &TaxSummary) {
    theme::boxed_flush(ui, |ui| {
        theme::grid_cell(ui, true, true, None, CELL_PAD, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(dim("ESTIMATED DEDUCTIBLE LOSSES (ONLY IF YOU ITEMIZE)").small());
            ui.label(
                RichText::new(money(s.deductible_losses()))
                    .size(22.0)
                    .family(theme::bold()),
            );
            ui.label(dim(format!(
                "{}% of {} in purchases, but never more than the {} you redeemed. With the \
                 standard deduction, losses don't reduce what you owe on redemptions.",
                s.loss_percent(),
                money(s.purchases),
                money(s.redeemed),
            )));
        });
    });
    ui.add_space(theme::SHADOW);
}
