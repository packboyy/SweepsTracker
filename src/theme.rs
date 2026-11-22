//! Flat two-blue look: white background, periwinkle primary, light-blue accent,
//! Oxanium font, square 2px borders, hard (unblurred) offset shadows, and the
//! underline/tab bar header.

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Rect, Response, Rounding,
    Sense, Shadow, Shape, Stroke, TextStyle, Vec2,
};

pub const BG: Color32 = Color32::WHITE;
/// Main content area, tinted so the white sidebar and white boxes stand out against it.
pub const CONTENT_BG: Color32 = Color32::from_rgb(0xdc, 0xdf, 0xfa);
/// Hovered tinted row (e.g. an addable casino in the dropdown).
pub const CONTENT_BG_HOVER: Color32 = Color32::from_rgb(0xc8, 0xcd, 0xf7);
pub const PRIMARY: Color32 = Color32::from_rgb(0x64, 0x64, 0xff);
/// Bars, rules and progress tracks only. Too pale for text on white.
pub const ACCENT: Color32 = Color32::from_rgb(0x64, 0xbf, 0xf0);
/// Secondary text: a softer, slightly grayed periwinkle; light but still readable on white.
pub const DIM: Color32 = Color32::from_rgb(0x73, 0x77, 0xd9);
/// Losses only; kept in the same soft, saturated family as the two blues.
pub const LOSS: Color32 = Color32::from_rgb(0xe0, 0x24, 0x50);
/// Gains. A clear green so profit never reads as just another blue number.
pub const GAIN: Color32 = Color32::from_rgb(0x0f, 0x9d, 0x58);
pub const STRIPE: Color32 = Color32::from_rgb(0xf0, 0xf2, 0xff);
pub const PAD: f32 = 5.0;
pub const BORDER: f32 = 2.0;
pub const MARGIN: f32 = 10.0;
/// Hard drop shadow offset (no blur).
pub const SHADOW: f32 = 4.0;

const BOLD: &str = "bold";

pub fn bold() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

fn hard_shadow(offset: f32, color: Color32) -> Shadow {
    Shadow {
        offset: Vec2::splat(offset),
        blur: 0.0,
        spread: 0.0,
        color,
    }
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Static weights: egui can't pick a weight from a variable font and renders it hairline-thin.
    // Oxanium has a 0.25em line gap that egui adds below the text, so glyphs sit high in every
    // widget. Shifting them down 0.16em centers them (0.18 centers caps, 0.10 x-height).
    let centered = |bytes: &'static [u8]| {
        FontData::from_static(bytes).tweak(egui::FontTweak {
            y_offset_factor: 0.16,
            ..Default::default()
        })
    };
    fonts.font_data.insert(
        "oxanium".into(),
        centered(include_bytes!("../assets/Oxanium-Medium.ttf")),
    );
    fonts.font_data.insert(
        "oxanium-semibold".into(),
        centered(include_bytes!("../assets/Oxanium-SemiBold.ttf")),
    );
    // egui's bundled fonts stay as fallbacks for symbols Oxanium lacks.
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "oxanium".into());
    let mut bold_family = vec!["oxanium-semibold".to_string()];
    bold_family.extend(fallbacks);
    fonts.families.insert(bold(), bold_family);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(28.0, bold())),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::new(15.0, bold())),
        (TextStyle::Small, FontId::new(12.0, bold())),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    // Dropdown / popup menus: rows run edge to edge, just inside the 2px border.
    style.spacing.menu_margin = egui::Margin::same(BORDER);

    let v = &mut style.visuals;
    v.override_text_color = Some(PRIMARY);
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = STRIPE;
    v.code_bg_color = STRIPE;
    v.hyperlink_color = PRIMARY;
    v.warn_fg_color = LOSS;
    v.error_fg_color = LOSS;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(BORDER, PRIMARY);
    v.text_cursor.stroke = Stroke::new(BORDER, PRIMARY);
    v.window_rounding = Rounding::ZERO;
    v.menu_rounding = Rounding::ZERO;
    v.window_shadow = hard_shadow(6.0, PRIMARY);
    v.popup_shadow = hard_shadow(SHADOW, PRIMARY);
    v.window_stroke = Stroke::new(BORDER, PRIMARY);

    let w = &mut v.widgets;
    for (state, fill) in [
        (&mut w.noninteractive, BG),
        (&mut w.inactive, BG),
        (&mut w.hovered, STRIPE),
        (&mut w.active, ACCENT),
        (&mut w.open, STRIPE),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(BORDER, PRIMARY);
        state.fg_stroke = Stroke::new(1.5_f32, PRIMARY);
        state.rounding = Rounding::ZERO;
        state.expansion = 0.0;
    }
    ctx.set_style(style);
}

/// Draws a widget with a hard shadow behind it. The shadow shrinks on hover/press so
/// buttons feel like they push in.
pub fn shadowed(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> Response) -> Response {
    shadowed_in(ui, PRIMARY, add)
}

/// `shadowed` with the shadow in `color` (e.g. red for a destructive button).
pub fn shadowed_in(
    ui: &mut egui::Ui,
    color: Color32,
    add: impl FnOnce(&mut egui::Ui) -> Response,
) -> Response {
    let slot = ui.painter().add(Shape::Noop);
    let resp = add(ui);
    let offset = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    ui.painter().set(
        slot,
        Shape::rect_filled(resp.rect.translate(Vec2::splat(offset)), 0.0, color),
    );
    resp
}

/// Button whose border, shadow and text all use `color`; `filled` makes it solid `color`
/// with white text. Used for destructive (red) and call-to-action buttons so the button
/// is one color instead of colored text in a periwinkle box.
pub fn colored_button(ui: &mut egui::Ui, text: &str, color: Color32, filled: bool) -> Response {
    let (fill, text_color) = if filled { (color, BG) } else { (BG, color) };
    let button = egui::Button::new(egui::RichText::new(text).color(text_color))
        .fill(fill)
        .stroke(Stroke::new(BORDER, color));
    shadowed_in(ui, color, |ui| ui.add(button)).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> Response {
    button_enabled(ui, true, text)
}

/// Shadowed button that can be disabled; shows a hand cursor when it can be clicked.
pub fn button_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    text: impl Into<egui::WidgetText>,
) -> Response {
    let text = text.into();
    let resp = shadowed(ui, |ui| ui.add_enabled(enabled, egui::Button::new(text)));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// White box with a 2px border and a hard shadow, both in `color`.
pub fn panel(color: Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(BG)
        .stroke(Stroke::new(BORDER, color))
        .shadow(hard_shadow(SHADOW, color))
        .inner_margin(egui::Margin::same(MARGIN))
}

/// A primary-colored `panel` that fills the available width exactly (leaving room for its shadow).
pub fn boxed<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    boxed_colored(ui, PRIMARY, add)
}

/// Sizing from outside the frame avoids the frame growing by its own margins every frame.
pub fn boxed_colored<R>(
    ui: &mut egui::Ui,
    color: Color32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let inner_width = (ui.available_width() - 2.0 * (MARGIN + BORDER) - SHADOW).max(0.0);
    panel(color)
        .show(ui, |ui| {
            ui.set_width(inner_width);
            add(ui)
        })
        .inner
}

/// Light-blue bar with a primary segment marking the selected slot (the header underline).
fn underline_bar(ui: &egui::Ui, rect: Rect, segment_x: f32, segment_w: f32) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, ACCENT);
    let seg = Rect::from_min_size(
        egui::pos2(rect.left() + segment_x, rect.top()),
        Vec2::new(segment_w, rect.height()),
    );
    p.rect_filled(seg, 0.0, PRIMARY);
}

/// Underline of a fixed width (for auto-sized windows, where "full width" would stretch
/// the window).
pub fn underline_width(ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, PAD), Sense::hover());
    underline_bar(ui, rect, 0.0, rect.width() / 3.0);
}

/// Full-width underline under a page title.
pub fn underline(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), PAD), Sense::hover());
    underline_bar(ui, rect, 0.0, rect.width() / 3.0);
}

/// Section title with a rule under it.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .family(bold())
            .size(14.0)
            .color(PRIMARY),
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), BORDER), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, PRIMARY);
    ui.add_space(4.0);
}

/// Tab labels over the underline bar; the primary segment slides to the selected tab.
pub fn tab_bar(
    ui: &mut egui::Ui,
    id: &str,
    selected: &mut usize,
    labels: &[&str],
    width: f32,
) -> Response {
    let font = FontId::new(15.0, bold());
    let height = font.size + 8.0 + PAD;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let seg_w = width / labels.len() as f32;
    if let (true, Some(pos)) = (resp.clicked(), resp.interact_pointer_pos()) {
        *selected = (((pos.x - rect.left()) / seg_w) as usize).min(labels.len() - 1);
    }
    let x = ui
        .ctx()
        .animate_value_with_time(ui.id().with(id), *selected as f32 * seg_w, 0.12);
    for (i, label) in labels.iter().enumerate() {
        let color = if i == *selected { PRIMARY } else { DIM };
        let pos = egui::pos2(rect.left() + seg_w * (i as f32 + 0.5), rect.top());
        ui.painter()
            .text(pos, Align2::CENTER_TOP, *label, font.clone(), color);
    }
    let bar = Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - PAD),
        Vec2::new(width, PAD),
    );
    underline_bar(ui, bar, x, seg_w);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sidebar row drawn as a button: bold text, 2px border, hard shadow that presses in on
/// hover/click, and a ▸ arrow. The selected row is filled primary with white text.
/// An optional logo URL is drawn before the text.
pub fn list_item(
    ui: &mut egui::Ui,
    text: &str,
    selected: bool,
    logo_url: Option<&str>,
    trailing: Option<(String, Color32)>,
) -> Response {
    let font = FontId::new(15.0, bold());
    // Leave room on the right for the shadow so it isn't clipped by the panel.
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - 4.0, font.size + 16.0),
        Sense::click(),
    );
    let offset = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    let (fill, text_color) = match (selected, resp.hovered()) {
        (true, _) => (PRIMARY, BG),
        (false, true) => (STRIPE, PRIMARY),
        (false, false) => (BG, PRIMARY),
    };
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(offset)), 0.0, PRIMARY);
    p.rect_filled(rect, 0.0, fill);
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, PRIMARY));

    let mut x = rect.left() + 8.0;
    if let Some(url) = logo_url {
        let size = font.size + 6.0;
        let logo_rect = Rect::from_min_size(
            egui::pos2(x, rect.center().y - size / 2.0),
            Vec2::splat(size),
        );
        logo_at(ui, url, logo_rect);
        x += size + 8.0;
    }
    p.text(
        egui::pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        text_color,
    );

    // ▸ arrow at the right edge.
    let arrow_x = rect.right() - 12.0;
    let (cy, h) = (rect.center().y, 5.0);
    p.add(Shape::convex_polygon(
        vec![
            egui::pos2(arrow_x - h * 0.8, cy - h),
            egui::pos2(arrow_x + h * 0.6, cy),
            egui::pos2(arrow_x - h * 0.8, cy + h),
        ],
        text_color,
        Stroke::NONE,
    ));
    if let Some((s, c)) = trailing {
        let small = TextStyle::Small.resolve(ui.style());
        let color = if selected { BG } else { c };
        p.text(
            egui::pos2(arrow_x - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            s,
            small,
            color,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sidebar card showing the open project ("PROJECT" caption over its name) with a ▾ arrow;
/// clicking it opens the project menu.
pub fn project_button(ui: &mut egui::Ui, name: &str) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width() - 4.0, 46.0), Sense::click());
    let offset = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(offset)), 0.0, PRIMARY);
    p.rect_filled(rect, 0.0, if resp.hovered() { STRIPE } else { BG });
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, PRIMARY));
    let text_area = Rect::from_min_max(rect.min, egui::pos2(rect.right() - 26.0, rect.bottom()));
    let p = p.with_clip_rect(text_area);
    p.text(
        rect.left_top() + Vec2::new(10.0, 7.0),
        Align2::LEFT_TOP,
        "PROJECT",
        FontId::new(10.5, bold()),
        DIM,
    );
    p.text(
        rect.left_bottom() + Vec2::new(10.0, -7.0),
        Align2::LEFT_BOTTOM,
        name,
        FontId::new(15.0, bold()),
        PRIMARY,
    );
    // ▾ arrow
    let (cx, cy, h) = (rect.right() - 14.0, rect.center().y, 5.0);
    ui.painter().add(Shape::convex_polygon(
        vec![
            egui::pos2(cx - h, cy - h * 0.6),
            egui::pos2(cx + h, cy - h * 0.6),
            egui::pos2(cx, cy + h * 0.6),
        ],
        PRIMARY,
        Stroke::NONE,
    ));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Height of a text input (`input`): the minimum text-edit height plus the 6 pt padding
/// above and below. Dropdowns and the primary button use it so a form row lines up.
pub fn control_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y + 12.0
}

/// Dropdown button drawn at exactly `width` (egui's ComboBox doesn't keep to a given width):
/// white with a 2px border, hard shadow that presses in, value on the left and ▾ on the right.
pub fn select_button(ui: &mut egui::Ui, text: &str, width: f32) -> Response {
    let height = control_height(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let offset = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(offset)), 0.0, PRIMARY);
    p.rect_filled(rect, 0.0, if resp.hovered() { STRIPE } else { BG });
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, PRIMARY));
    // Text runs up to just before the arrow.
    let clip = Rect::from_min_max(rect.min, egui::pos2(rect.right() - 18.0, rect.bottom()));
    p.with_clip_rect(clip).text(
        rect.left_center() + Vec2::new(8.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::new(14.0, bold()),
        PRIMARY,
    );
    let (cx, cy, h) = (rect.right() - 10.0, rect.center().y, 4.0);
    p.add(Shape::convex_polygon(
        vec![
            egui::pos2(cx - h, cy - h * 0.6),
            egui::pos2(cx + h, cy - h * 0.6),
            egui::pos2(cx, cy + h * 0.6),
        ],
        PRIMARY,
        Stroke::NONE,
    ));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square button with an X drawn as two lines (the ✕ character isn't in the font and
/// renders as an empty box). `color` is the X; `frame` the border and hard shadow.
pub fn x_button(ui: &mut egui::Ui, color: Color32, frame: Color32, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let offset = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(offset)), 0.0, frame);
    p.rect_filled(rect, 0.0, if resp.hovered() { STRIPE } else { BG });
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, frame));
    let c = rect.center();
    let r = size * 0.18;
    let stroke = Stroke::new(2.0_f32, color);
    p.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
    p.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Option row in a dropdown: the current choice is filled primary with white text (like
/// the selected sidebar row); others get a clear tint on hover.
pub fn menu_option(ui: &mut egui::Ui, label: &str, selected: bool) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    let text_color = if selected {
        p.rect_filled(rect, 0.0, PRIMARY);
        BG
    } else {
        if resp.hovered() {
            p.rect_filled(rect, 0.0, CONTENT_BG_HOVER);
        }
        PRIMARY
    };
    p.text(
        rect.left_center() + Vec2::new(12.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.5, bold()),
        text_color,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Full-width row in a dropdown menu: bold label, optional shortcut on the right.
pub fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 0.0, CONTENT_BG_HOVER);
    }
    p.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.5, bold()),
        PRIMARY,
    );
    if !shortcut.is_empty() {
        p.text(
            rect.right_center() - Vec2::new(10.0, 0.0),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::new(12.5, bold()),
            DIM,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Small drawn icons for the sidebar page links (drawn as shapes, so no font glyphs needed).
#[derive(Clone, Copy)]
pub enum NavIcon {
    Overview,
    Taxes,
    Trash,
}

fn paint_nav_icon(p: &egui::Painter, icon: NavIcon, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.8_f32, color);
    match icon {
        // Three bars of a chart.
        NavIcon::Overview => {
            for (i, h) in [0.45, 0.8, 0.6].into_iter().enumerate() {
                let w = rect.width() / 4.0;
                let x = rect.left() + i as f32 * (w + w / 2.0);
                let bar = Rect::from_min_max(
                    egui::pos2(x, rect.bottom() - rect.height() * h),
                    egui::pos2(x + w, rect.bottom()),
                );
                p.rect_filled(bar, 0.0, color);
            }
        }
        // A receipt: outlined page with two lines of text.
        NavIcon::Taxes => {
            let page = rect.shrink2(Vec2::new(rect.width() * 0.15, 0.0));
            p.rect_stroke(page.shrink(0.9), 0.0, stroke);
            for f in [0.38, 0.62] {
                let y = page.top() + page.height() * f;
                p.line_segment(
                    [
                        egui::pos2(page.left() + 3.5, y),
                        egui::pos2(page.right() - 3.5, y),
                    ],
                    stroke,
                );
            }
        }
        // A bin: lid line, handle, and body.
        NavIcon::Trash => {
            let lid_y = rect.top() + rect.height() * 0.22;
            p.line_segment(
                [
                    egui::pos2(rect.left(), lid_y),
                    egui::pos2(rect.right(), lid_y),
                ],
                stroke,
            );
            let handle = Rect::from_min_max(
                egui::pos2(rect.center().x - rect.width() * 0.18, rect.top()),
                egui::pos2(rect.center().x + rect.width() * 0.18, lid_y),
            );
            p.rect_stroke(handle, 0.0, stroke);
            let body = Rect::from_min_max(
                egui::pos2(rect.left() + rect.width() * 0.15, lid_y),
                egui::pos2(rect.right() - rect.width() * 0.15, rect.bottom()),
            );
            p.rect_stroke(body.shrink(0.9), 0.0, stroke);
        }
    }
}

/// Sidebar page link (Overview, Taxes, Recently deleted): a flat row with an icon, unlike
/// the bordered casino cards. Tinted on hover, filled primary when it's the current page.
pub fn nav_item(
    ui: &mut egui::Ui,
    icon: NavIcon,
    text: &str,
    selected: bool,
    trailing: Option<String>,
) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width() - 4.0, 34.0), Sense::click());
    let p = ui.painter();
    let color = if selected {
        p.rect_filled(rect, 0.0, PRIMARY);
        BG
    } else {
        if resp.hovered() {
            p.rect_filled(rect, 0.0, CONTENT_BG_HOVER);
        }
        PRIMARY
    };
    let icon_rect =
        Rect::from_center_size(rect.left_center() + Vec2::new(18.0, 0.0), Vec2::splat(15.0));
    paint_nav_icon(p, icon, icon_rect, color);
    p.text(
        rect.left_center() + Vec2::new(36.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::new(15.0, bold()),
        color,
    );
    if let Some(t) = trailing {
        // Count badge, e.g. how many items are in Recently deleted.
        let font = FontId::new(12.0, bold());
        let galley = p.layout_no_wrap(t, font, if selected { PRIMARY } else { BG });
        let badge = Rect::from_center_size(
            rect.right_center() - Vec2::new(10.0 + galley.size().x / 2.0 + 6.0, 0.0),
            galley.size() + Vec2::new(12.0, 4.0),
        );
        p.rect_filled(badge, 0.0, if selected { BG } else { PRIMARY });
        p.galley(badge.center() - galley.size() / 2.0, galley, PRIMARY);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Casino logo (loaded from the web and cached by egui) inside a 1px primary border.
pub fn logo_at(ui: &egui::Ui, url: &str, rect: Rect) {
    egui::Image::new(url).paint_at(ui, rect);
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, PRIMARY));
}

pub fn logo(ui: &mut egui::Ui, url: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    logo_at(ui, url, rect);
    resp
}

/// SweepsGrail community tier (S best .. C worst) as a filled square badge.
pub fn tier_badge(ui: &mut egui::Ui, tier: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_tier(ui.painter(), rect, tier);
    resp.on_hover_text(format!("SweepsGrail community tier: {tier}"))
}

pub fn paint_tier(p: &egui::Painter, rect: Rect, tier: &str) {
    let fill = match tier {
        "S" => GAIN,
        "A" => PRIMARY,
        "B" => DIM,
        _ => Color32::from_rgb(0x8a, 0x8f, 0xb8),
    };
    p.rect_filled(rect.translate(Vec2::splat(2.0)), 0.0, PRIMARY);
    p.rect_filled(rect, 0.0, fill);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        tier,
        FontId::new(rect.height() * 0.6, bold()),
        BG,
    );
}

/// Small outlined tag, e.g. "NEW".
pub fn tag(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .stroke(Stroke::new(BORDER, color))
        .inner_margin(egui::Margin::symmetric(5.0, 1.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).small().color(color));
        });
}

/// Thin grid lines between info tiles.
pub const GRID: Color32 = Color32::from_rgb(0xb9, 0xbc, 0xf5);

/// Like `boxed`, but with no inner padding: content runs right up to the 2px border.
pub fn boxed_flush<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner_width = (ui.available_width() - 2.0 * BORDER - SHADOW).max(0.0);
    panel(PRIMARY)
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(inner_width);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            add(ui)
        })
        .inner
}

/// One cell of a flush grid (see `boxed_flush`). Cells sit edge to edge; each draws a thin
/// line on its left/top unless it's in the first column/row, where the box's own border is.
pub fn grid_cell<R>(
    ui: &mut egui::Ui,
    first_col: bool,
    first_row: bool,
    fill: Option<Color32>,
    pad: Vec2,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let width = ui.available_width();
    let inner = egui::Frame::none()
        .fill(fill.unwrap_or(Color32::TRANSPARENT))
        .inner_margin(egui::Margin::symmetric(pad.x, pad.y))
        .show(ui, |ui| {
            ui.set_width(width - 2.0 * pad.x);
            add(ui)
        });
    let r = inner.response.rect;
    let stroke = Stroke::new(1.0_f32, GRID);
    if !first_col {
        ui.painter()
            .line_segment([r.left_top(), r.left_bottom()], stroke);
    }
    if !first_row {
        ui.painter()
            .line_segment([r.left_top(), r.right_top()], stroke);
    }
    inner
}

/// Compact tile for a flush grid: small label over a bold value. The value truncates (full
/// text plus `detail` on hover) so every tile in a row is the same height.
pub fn info_tile(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    detail: &str,
    first_col: bool,
    first_row: bool,
) -> Response {
    let value = if value.is_empty() { "—" } else { value };
    let resp = grid_cell(ui, first_col, first_row, None, Vec2::new(8.0, 4.0), |ui| {
        let line = |ui: &mut egui::Ui, text: egui::RichText| {
            ui.add(egui::Label::new(text).truncate());
        };
        line(
            ui,
            egui::RichText::new(label)
                .size(10.5)
                .family(bold())
                .color(DIM),
        );
        line(
            ui,
            egui::RichText::new(value)
                .size(15.0)
                .family(bold())
                .color(PRIMARY),
        );
    })
    .response;
    if detail.is_empty() {
        resp.on_hover_text(value)
    } else {
        resp.on_hover_text(format!("{value}\n{detail}"))
    }
}

/// Text input fill: a faint tint so fields read as "type here" rather than as buttons.
pub const INPUT_BG: Color32 = Color32::from_rgb(0xf7, 0xf8, 0xff);
pub const INPUT_BORDER: Color32 = Color32::from_rgb(0x9a, 0x9d, 0xe8);

/// Draws a text field as an input rather than a button: tinted fill, thin border, a soft
/// inset shadow along the top/left, and a primary border plus accent glow while focused.
/// `width` is the total outer width (use `f32::INFINITY` to fill).
pub fn input(ui: &mut egui::Ui, edit: egui::TextEdit<'_>, width: f32) -> Response {
    const PAD: egui::Vec2 = Vec2::new(8.0, 6.0);
    let slot = ui.painter().add(Shape::Noop);
    // TextEdit ignores its own margin when its frame is off, so the padding comes from a
    // wrapping Frame, and the box is drawn around that frame's rect.
    let shown = egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(PAD.x, PAD.y))
        .show(ui, |ui| {
            ui.add(edit.frame(false).desired_width(width - 2.0 * PAD.x))
        });
    let resp = shown.inner;
    let r = shown.response.rect;
    let focused = resp.has_focus();
    let inset = Color32::from_rgba_unmultiplied(0x64, 0x64, 0xff, 28);
    let border = if focused {
        Stroke::new(BORDER, PRIMARY)
    } else if ui.rect_contains_pointer(r) {
        Stroke::new(1.5_f32, PRIMARY)
    } else {
        Stroke::new(1.5_f32, INPUT_BORDER)
    };
    let mut shapes = Vec::new();
    if focused {
        shapes.push(Shape::rect_filled(
            r.expand(3.0),
            0.0,
            ACCENT.gamma_multiply(0.45),
        ));
    }
    shapes.push(Shape::rect_filled(r, 0.0, INPUT_BG));
    shapes.push(Shape::rect_filled(
        Rect::from_min_size(r.min, Vec2::new(r.width(), 3.0)),
        0.0,
        inset,
    ));
    shapes.push(Shape::rect_filled(
        Rect::from_min_size(r.min, Vec2::new(3.0, r.height())),
        0.0,
        inset,
    ));
    shapes.push(Shape::rect_stroke(
        r.shrink(border.width / 2.0),
        0.0,
        border,
    ));
    ui.painter().set(slot, Shape::Vec(shapes));
    resp
}

/// Solid primary call-to-action button (white text), with the usual hard shadow.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, text: &str, width: f32) -> Response {
    let button = egui::Button::new(egui::RichText::new(text).color(BG))
        .fill(PRIMARY)
        .min_size(Vec2::new(width, control_height(ui)));
    let resp = shadowed(ui, |ui| ui.add_enabled(enabled, button));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Progress bar: bordered accent track with a primary fill and a hard shadow.
pub fn progress(ui: &mut egui::Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SHADOW, 14.0),
        Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(3.0)), 0.0, PRIMARY);
    p.rect_filled(rect, 0.0, ACCENT);
    let fill = Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    p.rect_filled(fill, 0.0, PRIMARY);
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, PRIMARY));
}
