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
        // Hiding a casino from the list / putting a hidden one back (SweepsGrail slugs).
        let mut hide: Option<String> = None;
        let mut unhide: Option<String> = None;
        let hidden = &self.config.hidden_casinos;
        let show_hidden = self.picker.show_hidden;

        let query = grail::normalize(&self.picker.query);
        let typed = self.picker.query.trim().to_string();
        // (SweepsGrail slug, your casino's id) for casinos already in your list.
        let added: Vec<(&str, i64)> = self
            .data
            .casinos
            .iter()
            .filter_map(|c| {
                self.cache
                    .listing(&self.grail, c.id)
                    .map(|g| (g.slug.as_str(), c.id))
            })
            .collect();
        let added_id = |slug: &str| added.iter().find(|(s, _)| *s == slug).map(|(_, id)| *id);
        let mut matches: Vec<&GrailCasino> = self
            .grail
            .casinos
            .iter()
            .filter(|g| query.is_empty() || grail::normalize(&g.name).contains(&query))
            // The trash tab lists only hidden casinos; the normal list leaves them out.
            .filter(|g| hidden.contains(&g.slug) == show_hidden)
            .collect();
        if self.picker.by_rating {
            // Stable, so each tier keeps SweepsGrail's order.
            matches.sort_by_key(|g| tier_rank(&g.tier));
        }
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
                theme::panel(theme::border()).show(ui, |ui| {
                    ui.set_width(390.0);
                    let r = theme::input(
                        ui,
                        egui::TextEdit::singleline(&mut picker.query)
                            .hint_text(theme::hint_text("Search SweepsGrail casinos…")),
                        ui.available_width(),
                    );
                    if std::mem::take(&mut picker.focus) {
                        r.request_focus();
                    }
                    // Enter adds the first match that isn't added yet, else the typed name.
                    // Nothing typed: do nothing (it used to add the top casino by accident).
                    if submitted(ui, &r) && !typed.is_empty() && !show_hidden {
                        add = matches
                            .iter()
                            .find(|g| added_id(&g.slug).is_none())
                            .map(|g| g.name.clone())
                            .or_else(|| (!typed.is_empty() && !exact).then(|| typed.clone()));
                    }
                    ui.add_space(8.0);
                    let mut sort = usize::from(picker.by_rating);
                    ui.horizontal(|ui| {
                        // Room on the right for the trash tab (wider with a count).
                        let w = ui.available_width() - 70.0;
                        theme::tab_bar(
                            ui,
                            "picker_sort",
                            &mut sort,
                            &["Default order", "Best rated first"],
                            w,
                        )
                        .on_hover_text("Best rated: SweepsGrail community tier, S, A, B, then C");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(theme::SHADOW);
                            let icon = theme::NavIcon::Trash;
                            if theme::icon_button(ui, icon, hidden.len(), picker.show_hidden)
                                .on_hover_text(if picker.show_hidden {
                                    "Back to the list"
                                } else {
                                    "Casinos you've hidden from this list"
                                })
                                .clicked()
                            {
                                picker.show_hidden = !picker.show_hidden;
                            }
                        });
                    });
                    picker.by_rating = sort == 1;
                    ui.add_space(8.0);
                    if show_hidden {
                        ui.label(
                            dim_text("Hidden casinos. Click one to put it back in the list.")
                                .small(),
                        );
                        ui.add_space(4.0);
                    }
                    if !typed.is_empty() && !exact && !show_hidden {
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
                                let row = if show_hidden {
                                    RowKind::Hidden
                                } else if existing.is_some() {
                                    RowKind::Added
                                } else {
                                    RowKind::Addable
                                };
                                let (r, hide_clicked) = picker_row(ui, g, row);
                                if hide_clicked {
                                    hide = Some(g.slug.clone());
                                } else if r.clicked() {
                                    match (row, existing) {
                                        (RowKind::Hidden, _) => unhide = Some(g.slug.clone()),
                                        (_, Some(id)) => open = Some(id),
                                        (_, None) => add = Some(g.name.clone()),
                                    }
                                }
                            }
                            if matches.is_empty() && !loading {
                                ui.label(dim_text(if show_hidden && hidden.is_empty() {
                                    "Nothing hidden. Point at a casino in the list and click \
                                     its × to hide it here."
                                } else {
                                    "No SweepsGrail casinos match."
                                }));
                            }
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if loading {
                            ui.spinner();
                            ui.label(dim_text("Loading casinos from SweepsGrail…").small());
                        } else if let Some(e) = error {
                            ui.label(
                                RichText::new(format!("Couldn't load SweepsGrail: {e}"))
                                    .small()
                                    .color(loss()),
                            );
                        } else if fetched_at > 0 {
                            ui.label(
                                dim_text(format!(
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
        if hide.is_some() || unhide.is_some() {
            let result = self.config.update(|c| {
                if let Some(slug) = hide {
                    if !c.hidden_casinos.contains(&slug) {
                        c.hidden_casinos.push(slug);
                    }
                }
                if let Some(slug) = unhide {
                    c.hidden_casinos.retain(|s| *s != slug);
                }
            });
            if let Err(e) = result {
                self.status = Some(format!("Couldn't save the hidden casinos: {e}"));
            }
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

/// Sort position for a SweepsGrail tier: S, A, B, C, then unrated.
fn tier_rank(tier: &str) -> u8 {
    match tier {
        "S" => 0,
        "A" => 1,
        "B" => 2,
        "C" => 3,
        _ => 4,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RowKind {
    /// Not in your list yet: click adds it; × hides it.
    Addable,
    /// Already in your list: click opens it.
    Added,
    /// In the trash tab: click puts it back in the list.
    Hidden,
}

/// One casino in the "Add casino" dropdown: logo, name, signup bonus and redeem speed, tier.
/// Returns the row's response and whether its × (hide) was clicked.
fn picker_row(ui: &mut egui::Ui, g: &GrailCasino, kind: RowKind) -> (egui::Response, bool) {
    let added = kind != RowKind::Addable;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
    let p = ui.painter();
    // Casinos you can still add get a tinted background; ones already in your list stay white.
    let bg = match (added, resp.hovered()) {
        (false, false) => theme::content_bg(),
        (false, true) => theme::content_bg_hover(),
        (true, true) => theme::stripe(),
        (true, false) => theme::bg(),
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
    // × to hide, left of the tier badge; only while the pointer is over the row.
    let x_rect = egui::Rect::from_center_size(
        egui::pos2(badge.left() - 18.0, rect.center().y),
        Vec2::splat(20.0),
    );
    // Text stops short of the tier badge (and the ×) instead of running under them.
    let mut text_area = rect;
    text_area.set_right(if kind == RowKind::Addable {
        x_rect.left() - 6.0
    } else {
        badge.left() - 8.0
    });
    let p = &p.with_clip_rect(text_area);

    let x = logo.right() + 10.0;
    let name_color = if added { dim() } else { primary() };
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
        dim(),
    );

    if !g.tier.is_empty() {
        theme::paint_tier(ui.painter(), badge, &g.tier);
    }
    // ADDED / NEW tag sits right after the name, outlined like the casino page's NEW tag.
    let tag = if kind == RowKind::Added {
        Some(("ADDED", dim()))
    } else if g.is_new && kind == RowKind::Addable {
        Some(("NEW", gain()))
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

    let mut hide = false;
    if kind == RowKind::Addable && ui.rect_contains_pointer(rect) {
        let id = ui.id().with(("hide_casino", &g.slug));
        hide = theme::x_button_at(ui, x_rect, id, loss(), theme::grid())
            .on_hover_text("Hide from this list")
            .clicked();
    }

    let tip = match kind {
        RowKind::Added => "Already in your list. Click to open it.".to_string(),
        RowKind::Hidden => "Click to put it back in the list.".to_string(),
        RowKind::Addable if g.welcome_bonus.is_empty() => g.name.clone(),
        RowKind::Addable => g.welcome_bonus.clone(),
    };
    let resp = resp
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    (resp, hide)
}
