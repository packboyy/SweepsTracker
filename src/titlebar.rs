//! Custom title bar: a periwinkle strip (dark navy in dark mode) with the app and project name that you can drag
//! (double-click to maximize).
//!
//! - macOS: the native title bar is made transparent and the content extends under it, so
//!   the system's red/yellow/green buttons sit on top of this bar (left side reserved).
//! - Windows / Linux: the native frame is removed; this bar draws its own minimize,
//!   maximize and close buttons, and `resize_edges` restores drag-to-resize at the edges.

use crate::model::Appearance;
use crate::theme::{accent, bold, loss, titlebar_bg, titlebar_line, titlebar_text};
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Layout, Rect, ResizeDirection, Sense, Stroke,
    Vec2, ViewportCommand,
};

/// Periwinkle area plus the light-blue line under it. macOS centers its window buttons
/// 15 pt from the top, so a 30 pt periwinkle area puts them dead center.
pub const HEIGHT: f32 = BAR + LINE;
const BAR: f32 = 30.0;
const LINE: f32 = 3.0;
/// The bar text's center line, matching the macOS buttons. (Oxanium's glyphs sit about a
/// point above where egui centers them, hence the extra point.)
const TEXT_Y: f32 = BAR / 2.0 + 1.0;
/// Room for the macOS window buttons on the left.
const MAC_BUTTONS_WIDTH: f32 = 78.0;

/// Window options for the custom title bar on this platform.
pub fn configure(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        builder.with_decorations(false)
    }
}

/// What was clicked in the title bar this frame.
#[derive(Default)]
pub struct Clicks {
    /// The version, or the update notice in its place.
    pub version: bool,
    /// The light/dark button.
    pub appearance: bool,
}

/// Draws the title bar.
pub fn show(
    ctx: &egui::Context,
    project: Option<&str>,
    update: &crate::update::State,
    appearance: Appearance,
) -> Clicks {
    let mut clicks = Clicks::default();
    egui::TopBottomPanel::top("title_bar")
        .exact_height(HEIGHT)
        .frame(egui::Frame::none().fill(titlebar_bg()))
        .show_separator_line(false)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            // Background drag area first; the buttons added after it take priority.
            let drag = ui.interact(rect, ui.id().with("drag"), Sense::click_and_drag());
            if drag.double_clicked() {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }

            // Line along the bottom: light blue like the header underlines (muted in dark mode).
            let line = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - LINE), rect.max);
            ui.painter().rect_filled(line, 0.0, titlebar_line());

            let left = if cfg!(target_os = "macos") {
                MAC_BUTTONS_WIDTH
            } else {
                12.0
            };
            let p = ui.painter();
            let y = rect.top() + TEXT_Y;
            let title = p.text(
                egui::pos2(rect.left() + left, y),
                Align2::LEFT_CENTER,
                "SWEEPS TRACKER",
                FontId::new(13.0, bold()),
                titlebar_text(),
            );
            if let Some(name) = project {
                p.text(
                    egui::pos2(title.right() + 10.0, y),
                    Align2::LEFT_CENTER,
                    format!("·   {name}"),
                    FontId::new(13.0, bold()),
                    titlebar_text().gamma_multiply(0.7),
                );
            }

            // The version sits at the right edge (left of the window buttons off macOS).
            let mut right = rect.right() - 14.0;
            if !cfg!(target_os = "macos") {
                let bar = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + BAR));
                let buttons = egui::UiBuilder::new()
                    .max_rect(bar)
                    .layout(Layout::right_to_left(Align::Center));
                let placed = ui.allocate_new_ui(buttons, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    window_buttons(ui);
                });
                right = placed.response.rect.left() - 14.0;
            }
            // Light / dark / system button, left of the version.
            let button = Rect::from_center_size(egui::pos2(right - 10.0, y), Vec2::splat(20.0));
            let resp = ui.interact(button, ui.id().with("appearance"), Sense::click());
            paint_appearance(ui.painter(), button, appearance, resp.hovered());
            clicks.appearance = resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(format!(
                    "Appearance: {}. Click for {}.",
                    appearance.label(),
                    appearance.next().label()
                ))
                .clicked();
            right = button.left() - 12.0;

            // Click to check GitHub for a newer release; once one is found it takes the
            // version's place, and clicking it opens the release's download page.
            use crate::update::State;
            let current = format!(
                "v{} · build {}",
                env!("CARGO_PKG_VERSION"),
                env!("BUILD_NUMBER")
            );
            let (text, color, tip) = match update {
                State::Idle => (
                    current,
                    titlebar_text().gamma_multiply(0.7),
                    format!(
                        "Built from commit {}. Click to check for updates.",
                        env!("BUILD_COMMIT")
                    ),
                ),
                State::Checking => (
                    "Checking for updates…".into(),
                    titlebar_text().gamma_multiply(0.7),
                    "Asking GitHub for the latest release".into(),
                ),
                State::Available(release) => (
                    format!("{} available · Download", release.tag),
                    accent(),
                    format!("You have {current}. Click to open the release on GitHub."),
                ),
            };
            let label = ui.painter().text(
                egui::pos2(right, y),
                Align2::RIGHT_CENTER,
                text,
                FontId::new(12.0, bold()),
                color,
            );
            let resp = ui.interact(label.expand(3.0), ui.id().with("version"), Sense::click());
            if resp.hovered() && *update != State::Checking {
                ui.painter().hline(
                    label.x_range(),
                    label.bottom() + 1.0,
                    egui::Stroke::new(1.0_f32, color),
                );
            }
            clicks.version = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tip)
                .clicked();
        });
    clicks
}

/// Sun (light), moon (dark) or a half-filled circle (match system), in the bar's text color.
fn paint_appearance(p: &egui::Painter, rect: Rect, appearance: Appearance, hovered: bool) {
    if hovered {
        p.rect_filled(rect, 0.0, Color32::from_white_alpha(40));
    }
    let c = rect.center();
    let color = titlebar_text();
    let stroke = Stroke::new(1.5_f32, color);
    match appearance {
        Appearance::Light => {
            p.circle_filled(c, 3.5, color);
            for i in 0..8 {
                let dir = Vec2::angled(i as f32 * std::f32::consts::FRAC_PI_4);
                p.line_segment([c + dir * 5.5, c + dir * 7.5], stroke);
            }
        }
        Appearance::Dark => {
            // A full circle with a bar-colored one over its upper right: a crescent.
            p.circle_filled(c, 6.0, color);
            p.circle_filled(c + Vec2::new(3.0, -2.5), 5.0, titlebar_bg());
        }
        Appearance::System => {
            p.circle_stroke(c, 6.0, stroke);
            p.add(egui::Shape::convex_polygon(
                (0..=16)
                    .map(|i| {
                        let a =
                            std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 16.0;
                        c + Vec2::angled(a) * 6.0
                    })
                    .collect(),
                color,
                Stroke::NONE,
            ));
        }
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Close,
    Maximize,
    Minimize,
}

/// Close, maximize, minimize (laid out right to left).
fn window_buttons(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if icon_button(ui, Icon::Close).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
    if icon_button(ui, Icon::Maximize).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    if icon_button(ui, Icon::Minimize).clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

fn icon_button(ui: &mut egui::Ui, icon: Icon) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(44.0, ui.available_height()), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        let fill = match icon {
            Icon::Close => loss(),
            _ => Color32::from_white_alpha(40),
        };
        p.rect_filled(rect, 0.0, fill);
    }
    let c = rect.center();
    let s = 5.0;
    let stroke = Stroke::new(1.5_f32, titlebar_text());
    match icon {
        Icon::Close => {
            p.line_segment([c + Vec2::new(-s, -s), c + Vec2::new(s, s)], stroke);
            p.line_segment([c + Vec2::new(-s, s), c + Vec2::new(s, -s)], stroke);
        }
        Icon::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::splat(2.0 * s)), 0.0, stroke);
        }
        Icon::Minimize => {
            p.line_segment([c + Vec2::new(-s, 0.0), c + Vec2::new(s, 0.0)], stroke);
        }
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Windows / Linux: without the native frame, resize by dragging the window's edges and
/// corners. Call once per frame, after the panels.
pub fn resize_edges(ctx: &egui::Context) {
    if cfg!(target_os = "macos") {
        return;
    }
    const GRAB: f32 = 5.0;
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if maximized {
        return;
    }
    let screen = ctx.screen_rect();
    let (left, right) = (
        pos.x <= screen.left() + GRAB,
        pos.x >= screen.right() - GRAB,
    );
    let (top, bottom) = (
        pos.y <= screen.top() + GRAB,
        pos.y >= screen.bottom() - GRAB,
    );
    let direction = match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return,
    };
    let cursor = match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
