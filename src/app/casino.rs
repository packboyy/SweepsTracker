//! Casino page: header, SweepsGrail details, totals, new-entry form and entries table.

use super::*;

impl App {
    pub(super) fn casino_view(&mut self, ui: &mut egui::Ui, id: i64) {
        let Some(casino) = self.data.casino(id) else {
            self.view = View::Overview;
            return;
        };

        let info = self.cache.listing(&self.grail, id);
        // Buttons are laid out first (from the right) so they always get their space; the
        // logo, name and tier then fill what's left, and a long name truncates instead of
        // running under the buttons.
        let header = Vec2::new(ui.available_width(), 44.0);
        ui.allocate_ui_with_layout(header, Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(theme::SHADOW);
            if theme::colored_button(ui, "Delete", loss(), false)
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
                self.pasting = Some((id, String::new(), Vec::new()));
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
                        theme::tag(ui, "NEW", gain());
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
        stats_row(ui, self.stats_of(id));
        ui.add_space(10.0);

        self.daily_row(ui, id);
        ui.add_space(10.0);
        self.add_entry_form(ui, id);
        ui.add_space(10.0);
        self.entries_table(ui, id);
    }

    /// Today's daily login bonus: whether it's logged, the streak, and a one-click Claim that
    /// logs the same amount as last time.
    fn daily_row(&mut self, ui: &mut egui::Ui, id: i64) {
        let Some(cached) = self.cache.casino.get(&id) else {
            return;
        };
        let today = today();
        let days = &cached.daily_days;
        let claimed = days.contains(&today);
        let streak = streak(days, today);
        let last = cached.last_daily_sc;
        let mut claim = false;
        theme::boxed_flush(ui, |ui| {
            theme::grid_cell(ui, true, true, None, Vec2::new(CELL_PAD.x, 6.0), |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                // Button first (from the right) like the page header, then everything else
                // centred on the same line.
                let row = Vec2::new(ui.available_width(), 32.0);
                ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(theme::SHADOW);
                    if !claimed {
                        let (text, hint) = match last {
                            Some(sc) => (
                                format!("Claim {} SC", coins(sc)),
                                "Log today's daily login bonus (same amount as last time)",
                            ),
                            None => ("Claim daily".to_string(), "Log today's daily login bonus"),
                        };
                        claim = theme::colored_button(ui, &text, primary(), true)
                            .on_hover_text(hint)
                            .clicked();
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.label(dim_text("DAILY LOGIN").small());
                        let (status, color) = if claimed {
                            ("Claimed today", gain())
                        } else if days.is_empty() {
                            ("Not tracked yet", dim())
                        } else {
                            ("Not claimed yet today", primary())
                        };
                        ui.label(RichText::new(status).family(theme::bold()).color(color));
                        if streak > 0 {
                            let color = if claimed { gain() } else { primary() };
                            theme::tag(ui, &streak_text(streak), color);
                            if !claimed {
                                ui.add(
                                    egui::Label::new(
                                        dim_text("claim today to keep it going").small(),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                    });
                });
            });
        });
        ui.add_space(theme::SHADOW);
        if !claim {
            return;
        }
        match last {
            Some(sc) => {
                let result = self.db().add_free_coins(id, today, sc, Source::Daily, "");
                self.apply(result);
            }
            // First claim here: the amount isn't known yet, so fill in the form instead.
            None => {
                let f = &mut self.form;
                f.kind = Kind::FreeSc;
                f.fields.source = Source::Daily;
                f.fields.date = today;
                f.focus_first = true;
                self.status = Some(
                    "Enter how many SC today's daily gave you. Next time, Claim logs the same \
                     amount in one click."
                        .into(),
                );
            }
        }
    }

    fn add_entry_form(&mut self, ui: &mut egui::Ui, casino_id: i64) {
        let EntryForm {
            kind,
            fields: f,
            focus_first,
        } = &mut self.form;
        let mut submit = false;
        let valid = f.valid(*kind);
        // Flush grid like the boxes above: a top cell with the type tabs and date, then a row
        // of cells (label above field) ending with the Add button.
        theme::boxed_flush(ui, |ui| {
            theme::grid_cell(ui, true, true, None, CELL_PAD, |ui| {
                // The flush grid zeroes spacing; this row needs normal gaps between widgets.
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 6.0);
                ui.horizontal(|ui| {
                    let kinds = [Kind::Purchase, Kind::Withdrawal, Kind::FreeSc];
                    let mut tab = kinds.iter().position(|k| k == kind).unwrap_or(0);
                    theme::tab_bar(
                        ui,
                        "entry_kind",
                        &mut tab,
                        &["Purchase", "Withdrawal", "Free SC"],
                        // Narrower in a small window, leaving room for the date and Today.
                        (ui.available_width() - 300.0).clamp(270.0, 330.0),
                    );
                    *kind = kinds[tab];
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(theme::SHADOW);
                        if theme::button(ui, "Today").clicked() {
                            f.date = today();
                        }
                        theme::shadowed(ui, |ui| {
                            ui.add(date_picker(&mut f.date, "new_entry_date"))
                        });
                        ui.label(dim_text("DATE").small());
                    });
                });
            });

            let columns = match *kind {
                Kind::Purchase => 5,
                Kind::Withdrawal => 6,
                Kind::FreeSc => 4,
            };
            ui.columns(columns, |cols| {
                for col in cols.iter_mut() {
                    col.spacing_mut().item_spacing = Vec2::ZERO;
                }
                let first = match *kind {
                    Kind::Purchase => {
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
                    }
                    Kind::Withdrawal => {
                        let r = field_cell(&mut cols[0], true, "AMOUNT $", |ui| {
                            let w = fill_width(ui);
                            amount_field(ui, &mut f.amount, "500", w)
                        });
                        submit |= submitted(&cols[0], &r);
                        field_cell(&mut cols[1], false, "METHOD", |ui| {
                            let w = fill_width(ui);
                            choice(
                                ui,
                                "new_method",
                                &mut f.method,
                                &Method::ALL,
                                Method::label,
                                w,
                            )
                        });
                        let r2 = field_cell(&mut cols[2], false, "FEE $", |ui| {
                            let w = fill_width(ui);
                            amount_field(ui, &mut f.fee, "none", w)
                        });
                        submit |= submitted(&cols[2], &r2);
                        field_cell(&mut cols[3], false, "STATUS", |ui| {
                            let w = fill_width(ui);
                            choice(
                                ui,
                                "new_status",
                                &mut f.received,
                                &[true, false],
                                status_label,
                                w,
                            )
                        });
                        r
                    }
                    Kind::FreeSc => {
                        let r = field_cell(&mut cols[0], true, "SC", |ui| {
                            let w = fill_width(ui);
                            amount_field(ui, &mut f.sc, "1", w)
                        });
                        submit |= submitted(&cols[0], &r);
                        field_cell(&mut cols[1], false, "SOURCE", |ui| {
                            let w = fill_width(ui);
                            choice(
                                ui,
                                "new_source",
                                &mut f.source,
                                &Source::ALL,
                                Source::label,
                                w,
                            )
                        });
                        r
                    }
                };
                if *focus_first {
                    first.request_focus();
                    *focus_first = false;
                }

                let (note_col, add_col) = (columns - 2, columns - 1);
                let r = field_cell(&mut cols[note_col], false, "NOTE", |ui| {
                    let w = fill_width(ui);
                    theme::input(
                        ui,
                        egui::TextEdit::singleline(&mut f.note)
                            .hint_text(theme::hint_text("optional")),
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
        let (date, note) = (f.date, f.note.trim().to_string());
        let result = match *kind {
            Kind::Purchase => {
                let (spent, cc) = (cents(&f.spent), cents(&f.cc));
                self.db().add_purchase(casino_id, date, spent, cc, &note)
            }
            Kind::Withdrawal => {
                let r = f.redemption(f.received.then_some(date));
                self.db().add_redemption(casino_id, &r)
            }
            Kind::FreeSc => {
                let (sc, source) = (cents(&f.sc), f.source);
                self.db().add_free_coins(casino_id, date, sc, source, &note)
            }
        };
        // Keep what was typed if saving failed; otherwise clear the amounts for the next
        // entry (method, status and source stay as chosen).
        if self.apply(result).is_some() {
            self.form.focus_first = true;
            let f = &mut self.form.fields;
            for field in [
                &mut f.spent,
                &mut f.cc,
                &mut f.amount,
                &mut f.fee,
                &mut f.sc,
                &mut f.note,
            ] {
                field.clear();
            }
        }
    }

    fn entries_table(&mut self, ui: &mut egui::Ui, casino_id: i64) {
        let (Some(casino), Some(cached)) = (
            self.data.casino(casino_id),
            self.cache.casino.get(&casino_id),
        ) else {
            return;
        };
        if cached.rows.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(dim_text(
                    "No entries yet. Add one above, or paste rows from your spreadsheet.",
                ));
            });
            return;
        }

        let rows = &cached.rows;
        let mut edit = None;
        let mut delete = None;
        let mut mark_received = None;
        let right = Layout::right_to_left(Align::Center);

        // Leave room below the table for the box's margin, border and shadow.
        let max_height =
            (ui.available_height() - 2.0 * (theme::MARGIN + theme::BORDER) - theme::SHADOW - 4.0)
                .max(80.0);
        let small = |text: String, color: Color32| RichText::new(text).small().color(color);
        theme::boxed(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                // Not resizable: egui draws the resize handles as heavy column lines.
                .resizable(false)
                .sense(Sense::click())
                .max_scroll_height(max_height)
                .cell_layout(Layout::left_to_right(Align::Center))
                // Fixed columns + the details minimum add up to ~520, which fits beside the
                // widest sidebar at the smallest window size.
                .column(Column::exact(82.0))
                .column(Column::exact(74.0))
                .column(Column::exact(84.0))
                .column(Column::exact(56.0))
                .column(Column::exact(54.0))
                .column(Column::remainder().at_least(110.0).clip(true))
                .column(Column::exact(58.0))
                .header(22.0, |mut header| {
                    for (h, right_aligned) in [
                        ("DATE", false),
                        ("TYPE", false),
                        ("AMOUNT", true),
                        ("CC", true),
                        ("DISC.", true),
                        ("DETAILS", false),
                        ("", false),
                    ] {
                        header.col(|ui| {
                            let rect = ui.max_rect();
                            ui.painter().hline(
                                rect.x_range(),
                                rect.bottom(),
                                Stroke::new(1.0_f32, theme::grid()),
                            );
                            if right_aligned {
                                ui.with_layout(right, |ui| ui.label(dim_text(h).small()));
                            } else {
                                ui.label(dim_text(h).small());
                            }
                        });
                    }
                })
                // Only rows on screen are laid out, so long histories scroll smoothly.
                .body(|body| {
                    body.rows(26.0, rows.len(), |mut row| {
                        let r = &rows[row.index()].get(casino);
                        row.col(|ui| {
                            ui.label(r.date().format("%-m/%-d/%Y").to_string());
                        });
                        let (kind, kind_color) = match r {
                            Row::Purchase(_) => ("Purchase", primary()),
                            Row::Redemption(_) => ("Withdrawal", gain()),
                            Row::FreeCoins(_) => ("Free SC", accent()),
                        };
                        row.col(|ui| {
                            ui.label(small(kind.into(), kind_color));
                        });
                        // Amount, coins and discount; details are dim facts before the note.
                        let (amount, cc, discount, details) = match r {
                            Row::Purchase(p) => (
                                (p.spent != 0).then(|| RichText::new(money(p.spent))),
                                (p.cc_received != 0).then(|| coins(p.cc_received)),
                                p.discount().map(percent),
                                String::new(),
                            ),
                            Row::Redemption(red) => {
                                let mut details = red.method.label().to_string();
                                if red.fee > 0 {
                                    details += &format!(" · {} fee", money(red.fee));
                                }
                                if let Some(d) = red.received {
                                    details += &format!(" · received {}", d.format("%-m/%-d/%Y"));
                                }
                                let amount = RichText::new(format!("+{}", money(red.amount)));
                                (Some(amount.color(gain())), None, None, details)
                            }
                            Row::FreeCoins(f) => (
                                None,
                                Some(format!("+{}", coins(f.sc))),
                                None,
                                f.source.label().to_string(),
                            ),
                        };
                        row.col(|ui| {
                            if let Some(amount) = amount {
                                ui.with_layout(right, |ui| ui.label(amount));
                            }
                        });
                        row.col(|ui| {
                            if let Some(cc) = cc {
                                ui.with_layout(right, |ui| ui.label(cc));
                            }
                        });
                        row.col(|ui| {
                            if let Some(d) = discount {
                                ui.with_layout(right, |ui| ui.label(dim_text(d)));
                            }
                        });
                        row.col(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if !details.is_empty() {
                                ui.label(dim_text(details));
                            }
                            if let Row::Redemption(red) = r {
                                if red.is_pending() {
                                    theme::tag(ui, "PENDING", loss());
                                    if theme::small_button(ui, "Mark received", primary())
                                        .on_hover_text("Record today as the date the money arrived")
                                        .clicked()
                                    {
                                        mark_received = Some(red.id);
                                    }
                                }
                            }
                            // The note gets what's left; hover shows it in full if cut off.
                            if !r.note().is_empty() {
                                ui.add(egui::Label::new(r.note()).truncate());
                            }
                        });
                        row.col(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ui.link(small("Edit".into(), primary())).clicked() {
                                edit = Some(r.edit_state());
                            }
                            if theme::x_button(ui, loss(), theme::grid(), 18.0)
                                .on_hover_text("Delete (you can restore it from Recently deleted)")
                                .clicked()
                            {
                                delete = Some(r.entry_ref());
                            }
                        });
                        if row.response().double_clicked() {
                            edit = Some(r.edit_state());
                        }
                    });
                });
        });

        if edit.is_some() {
            self.editing = edit;
        }
        if let Some(entry) = delete {
            let result = self.db().delete_entry(entry);
            if self.apply(result).is_some() {
                let what = match entry {
                    EntryRef::Purchase(_) => "a purchase",
                    EntryRef::Redemption(_) => "a withdrawal",
                    EntryRef::FreeCoins(_) => "a free SC entry",
                };
                self.deleted_notice(Undo::Entry(entry), what.to_string());
            }
        }
        if let Some(id) = mark_received {
            let result = self.db().mark_received(id, today());
            self.apply(result);
        }
    }
}

/// Where an entries-table row lives in its casino's lists (cached, so the table isn't
/// gathered and sorted every frame).
#[derive(Clone, Copy)]
pub(super) enum RowIndex {
    Purchase(usize),
    Redemption(usize),
    FreeCoins(usize),
}

impl RowIndex {
    fn get(self, casino: &Casino) -> Row<'_> {
        match self {
            RowIndex::Purchase(i) => Row::Purchase(&casino.purchases[i]),
            RowIndex::Redemption(i) => Row::Redemption(&casino.redemptions[i]),
            RowIndex::FreeCoins(i) => Row::FreeCoins(&casino.free_coins[i]),
        }
    }
}

/// Purchases, redemptions and free coins in one list, newest first, like the spreadsheet.
pub(super) fn sorted_rows(casino: &Casino) -> Vec<RowIndex> {
    let mut rows: Vec<RowIndex> = (0..casino.purchases.len())
        .map(RowIndex::Purchase)
        .chain((0..casino.redemptions.len()).map(RowIndex::Redemption))
        .chain((0..casino.free_coins.len()).map(RowIndex::FreeCoins))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.get(casino).sort_key()));
    rows
}

/// A purchase, redemption or free-coins entry, for showing them all in one table.
enum Row<'a> {
    Purchase(&'a Purchase),
    Redemption(&'a Redemption),
    FreeCoins(&'a FreeCoins),
}

impl Row<'_> {
    fn date(&self) -> NaiveDate {
        match self {
            Row::Purchase(p) => p.date,
            Row::Redemption(r) => r.date,
            Row::FreeCoins(f) => f.date,
        }
    }

    fn note(&self) -> &str {
        match self {
            Row::Purchase(p) => &p.note,
            Row::Redemption(r) => &r.note,
            Row::FreeCoins(f) => &f.note,
        }
    }

    fn entry_ref(&self) -> EntryRef {
        match self {
            Row::Purchase(p) => EntryRef::Purchase(p.id),
            Row::Redemption(r) => EntryRef::Redemption(r.id),
            Row::FreeCoins(f) => EntryRef::FreeCoins(f.id),
        }
    }

    /// Date, then purchases / redemptions / free coins on the same day, then entry order.
    fn sort_key(&self) -> (NaiveDate, u8, i64) {
        match self {
            Row::Purchase(p) => (p.date, 0, p.id),
            Row::Redemption(r) => (r.date, 1, r.id),
            Row::FreeCoins(f) => (f.date, 2, f.id),
        }
    }

    fn edit_state(&self) -> EditState {
        let blank = EntryFields {
            note: self.note().to_string(),
            ..EntryFields::new(self.date())
        };
        let mut received_date = self.date();
        let fields = match self {
            Row::Purchase(p) => EntryFields {
                spent: edit_text(p.spent),
                cc: edit_text(p.cc_received),
                ..blank
            },
            Row::Redemption(r) => {
                received_date = r.received.unwrap_or_else(today);
                EntryFields {
                    amount: edit_text(r.amount),
                    method: r.method,
                    fee: edit_text(r.fee),
                    received: r.received.is_some(),
                    ..blank
                }
            }
            Row::FreeCoins(f) => EntryFields {
                sc: edit_text(f.sc),
                source: f.source,
                ..blank
            },
        };
        EditState {
            row: self.entry_ref(),
            fields,
            received_date,
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
