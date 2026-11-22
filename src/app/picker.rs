//! "Add casino" dropdown backed by the SweepsGrail directory.

use super::*;

impl App {
    /// "Add casino" dropdown: search SweepsGrail's list, or add a custom name.
    pub(super) fn casino_picker(&mut self, ctx: &egui::Context) {
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
