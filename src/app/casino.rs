//! Casino page: header, SweepsGrail details, totals, new-entry form and entries table.

use super::*;

impl App {
    pub(super) fn casino_view(&mut self, ui: &mut egui::Ui, id: i64) {
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
