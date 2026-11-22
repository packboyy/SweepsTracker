//! Custom title bar: a periwinkle strip with the app and project name that you can drag
//! (double-click to maximize).
//!
//! - macOS: the native title bar is made transparent and the content extends under it, so
//!   the system's red/yellow/green buttons sit on top of this bar (left side reserved).
//! - Windows / Linux: the native frame is removed; this bar draws its own minimize,
//!   maximize and close buttons, and `resize_edges` restores drag-to-resize at the edges.

use crate::theme::{bold, ACCENT, BG, LOSS, PRIMARY};
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

pub fn show(ctx: &egui::Context, project: Option<&str>) {
    egui::TopBottomPanel::top("title_bar")
        .exact_height(HEIGHT)
        .frame(egui::Frame::none().fill(PRIMARY))
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

            // Light-blue line along the bottom, like the header underlines.
            let line = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - LINE), rect.max);
            ui.painter().rect_filled(line, 0.0, ACCENT);

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
                BG,
            );
            if let Some(name) = project {
                p.text(
                    egui::pos2(title.right() + 10.0, y),
                    Align2::LEFT_CENTER,
                    format!("·   {name}"),
                    FontId::new(13.0, bold()),
                    BG.gamma_multiply(0.7),
                );
            }

            if !cfg!(target_os = "macos") {
                let bar = Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + BAR));
                let buttons = egui::UiBuilder::new()
                    .max_rect(bar)
                    .layout(Layout::right_to_left(Align::Center));
                ui.allocate_new_ui(buttons, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    window_buttons(ui);
                });
            }
        });
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
            Icon::Close => LOSS,
            _ => Color32::from_white_alpha(40),
        };
        p.rect_filled(rect, 0.0, fill);
    }
    let c = rect.center();
    let s = 5.0;
    let stroke = Stroke::new(1.5_f32, BG);
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
