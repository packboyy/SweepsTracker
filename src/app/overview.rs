//! Overview page: totals, profit chart and the by-casino table.

use super::*;

impl App {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Overview", |_| {});
        stats_row(ui, self.cache.totals);
        ui.add_space(16.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::section(ui, "Profit over time");
                profit_chart(ui, &self.cache.daily_net);
                ui.add_space(20.0);

                theme::section(ui, "By casino");
                let mut rows: Vec<(i64, &str, Stats, usize)> = self
                    .data
                    .casinos
                    .iter()
                    .map(|c| (c.id, c.name.as_str(), self.stats_of(c.id), c.entry_count()))
                    .collect();
                rows.sort_by(|a, b| b.2.net().cmp(&a.2.net()));

                // Flush grid with equal-width columns, so it always fits the box (a Grid with
                // minimum column widths ran off the right edge in narrow windows).
                let mut open = None;
                let pad = Vec2::new(10.0, 4.0);
                theme::boxed_flush(ui, |ui| {
                    let headers = [
                        "CASINO",
                        "ENTRIES",
                        "SPENT",
                        "CC RECEIVED",
                        "DISCOUNT",
                        "WITHDRAWN",
                        "NET",
                    ];
                    ui.columns(headers.len(), |cols| {
                        for (c, (col, h)) in cols.iter_mut().zip(headers).enumerate() {
                            col.spacing_mut().item_spacing = Vec2::ZERO;
                            theme::grid_cell(col, c == 0, true, None, pad, |ui| {
                                ui.add(egui::Label::new(dim(h).small()).truncate());
                            });
                        }
                    });
                    if rows.is_empty() {
                        theme::grid_cell(ui, true, false, None, Vec2::new(10.0, 12.0), |ui| {
                            ui.label(dim("No casinos yet. Add one from the sidebar."));
                        });
                    }
                    for (r, (id, name, s, count)) in rows.into_iter().enumerate() {
                        let fill = (r % 2 == 1).then_some(theme::STRIPE);
                        let cells = [
                            count.to_string(),
                            money(s.spent),
                            coins(s.cc_received),
                            s.avg_discount().map(percent).unwrap_or_else(|| "—".into()),
                            money(s.withdrawn),
                            signed_money(s.net()),
                        ];
                        ui.columns(cells.len() + 1, |cols| {
                            for col in cols.iter_mut() {
                                col.spacing_mut().item_spacing = Vec2::ZERO;
                            }
                            theme::grid_cell(&mut cols[0], true, false, fill, pad, |ui| {
                                let name_text =
                                    RichText::new(name).family(theme::bold()).underline();
                                let link = ui
                                    .add(
                                        egui::Label::new(name_text)
                                            .truncate()
                                            .sense(Sense::click()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if link.clicked() {
                                    open = Some(id);
                                }
                            });
                            let last = cells.len();
                            for (c, (col, text)) in cols[1..].iter_mut().zip(cells).enumerate() {
                                let color = if c + 1 == last {
                                    profit_color(s.net())
                                } else {
                                    PRIMARY
                                };
                                theme::grid_cell(col, false, false, fill, pad, |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(text).color(color))
                                            .truncate(),
                                    );
                                });
                            }
                        });
                    }
                });
                ui.add_space(theme::SHADOW + 4.0);
                if let Some(id) = open {
                    self.view = View::Casino(id);
                }
            });
    }
}

/// Line chart of cumulative net profit (redeemed - spent) by day. Values are cents.
fn profit_chart(ui: &mut egui::Ui, by_day: &BTreeMap<NaiveDate, Cents>) {
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width() - theme::SHADOW, 170.0),
        Sense::hover(),
    );
    let rect = resp.rect;
    painter.rect_filled(rect.translate(Vec2::splat(theme::SHADOW)), 0.0, PRIMARY);
    painter.rect_filled(rect, 0.0, theme::BG);
    painter.rect_stroke(
        rect.shrink(theme::BORDER / 2.0),
        0.0,
        Stroke::new(theme::BORDER, PRIMARY),
    );

    if by_day.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Add entries on at least two different days to see a chart",
            FontId::proportional(14.0),
            DIM,
        );
        return;
    }

    let mut total: Cents = 0;
    let points: Vec<(NaiveDate, Cents)> = by_day
        .iter()
        .map(|(&d, &v)| {
            total += v;
            (d, total)
        })
        .collect();
    let first = points[0].0;
    let span = (points.last().unwrap().0 - first).num_days().max(1) as f32;
    let min = points.iter().map(|p| p.1).min().unwrap_or(0).min(0) as f32;
    let max = points.iter().map(|p| p.1).max().unwrap_or(0).max(0) as f32;
    let range = (max - min).max(1.0);

    let plot = rect.shrink2(Vec2::new(64.0, 18.0));
    let to_screen = |d: NaiveDate, v: Cents| {
        let x = plot.left() + plot.width() * (d - first).num_days() as f32 / span;
        let y = plot.bottom() - plot.height() * (v as f32 - min) / range;
        egui::pos2(x, y)
    };

    // Zero line and axis labels
    let zero_y = to_screen(first, 0).y;
    painter.line_segment(
        [
            egui::pos2(plot.left(), zero_y),
            egui::pos2(plot.right(), zero_y),
        ],
        Stroke::new(theme::BORDER, ACCENT),
    );
    let small = FontId::proportional(11.5);
    for v in [max, 0.0, min] {
        let y = plot.bottom() - plot.height() * (v - min) / range;
        painter.text(
            egui::pos2(rect.left() + 8.0, y),
            Align2::LEFT_CENTER,
            money(v as Cents),
            small.clone(),
            DIM,
        );
    }
    let last = points.last().unwrap().0;
    painter.text(
        egui::pos2(plot.left(), rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        first.format("%-m/%-d").to_string(),
        small.clone(),
        DIM,
    );
    painter.text(
        egui::pos2(plot.right(), rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        last.format("%-m/%-d").to_string(),
        small.clone(),
        DIM,
    );

    let screen: Vec<egui::Pos2> = points.iter().map(|&(d, v)| to_screen(d, v)).collect();
    painter.add(egui::Shape::line(
        screen.clone(),
        Stroke::new(2.5_f32, PRIMARY),
    ));

    // Hover: highlight the nearest day
    if let Some(pos) = resp.hover_pos() {
        if let Some((i, p)) = screen
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.x - pos.x).abs().total_cmp(&(b.1.x - pos.x).abs()))
        {
            painter.rect_filled(
                egui::Rect::from_center_size(*p, Vec2::splat(8.0)),
                0.0,
                PRIMARY,
            );
            let (d, v) = points[i];
            let label = format!("{}  {}", d.format("%-m/%-d/%Y"), signed_money(v));
            let align = if p.x > rect.center().x {
                Align2::RIGHT_BOTTOM
            } else {
                Align2::LEFT_BOTTOM
            };
            painter.text(
                *p + Vec2::new(0.0, -10.0),
                align,
                label,
                FontId::proportional(13.0),
                profit_color(v),
            );
        }
    }
}
