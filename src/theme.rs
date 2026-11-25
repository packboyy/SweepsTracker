//! Flat two-blue look: white background, periwinkle primary, light-blue accent,
//! Oxanium font, square 2px borders, hard (unblurred) offset shadows, and the
//! underline/tab bar header.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Rect, Response, Rounding,
    Sense, Shadow, Shape, Stroke, TextStyle, Vec2,
};

/// Placeholder text in the hint color.
pub fn hint_text(text: &str) -> egui::RichText {
    egui::RichText::new(text).color(hint())
}

/// Every color the app draws with, in its light and dark versions.
struct Palette {
    /// Boxes, sidebar and inputs' surroundings; also the text on primary fills.
    bg: Color32,
    /// Main content area, tinted so the sidebar and boxes stand out against it.
    content_bg: Color32,
    /// Hovered tinted row (e.g. an addable casino in the dropdown).
    content_bg_hover: Color32,
    /// Placeholder text in inputs: washed out so it can't be mistaken for a typed value.
    hint: Color32,
    primary: Color32,
    /// Box and card outlines, section rules and hard shadows. The same as `primary` in light
    /// mode; darker than the text in dark mode, so outlines don't compete with it.
    border: Color32,
    /// The title bar strip, its text, and the line under it.
    titlebar_bg: Color32,
    titlebar_text: Color32,
    titlebar_line: Color32,
    /// Bars, rules and progress tracks only. Too pale for text on the background.
    accent: Color32,
    /// Secondary text: a softer, slightly grayed periwinkle; still readable on the background.
    dim: Color32,
    /// Losses only; kept in the same soft, saturated family as the two blues.
    loss: Color32,
    /// Gains. A clear green so profit never reads as just another blue number.
    gain: Color32,
    stripe: Color32,
    /// Thin grid lines between info tiles.
    grid: Color32,
    /// Text input fill: a faint tint so fields read as "type here" rather than as buttons.
    input_bg: Color32,
    input_border: Color32,
    /// The soft inset shadow along an input's top and left edges.
    input_inset: Color32,
    /// Lowest SweepsGrail tier's badge.
    tier_c: Color32,
}

const LIGHT: Palette = Palette {
    bg: Color32::WHITE,
    content_bg: Color32::from_rgb(0xdc, 0xdf, 0xfa),
    content_bg_hover: Color32::from_rgb(0xc8, 0xcd, 0xf7),
    hint: Color32::from_rgb(0xb4, 0xb7, 0xee),
    primary: Color32::from_rgb(0x64, 0x64, 0xff),
    border: Color32::from_rgb(0x64, 0x64, 0xff),
    titlebar_bg: Color32::from_rgb(0x64, 0x64, 0xff),
    titlebar_text: Color32::WHITE,
    titlebar_line: Color32::from_rgb(0x64, 0xbf, 0xf0),
    accent: Color32::from_rgb(0x64, 0xbf, 0xf0),
    dim: Color32::from_rgb(0x73, 0x77, 0xd9),
    loss: Color32::from_rgb(0xe0, 0x24, 0x50),
    gain: Color32::from_rgb(0x0f, 0x9d, 0x58),
    stripe: Color32::from_rgb(0xf0, 0xf2, 0xff),
    grid: Color32::from_rgb(0xb9, 0xbc, 0xf5),
    input_bg: Color32::from_rgb(0xf7, 0xf8, 0xff),
    input_border: Color32::from_rgb(0x9a, 0x9d, 0xe8),
    input_inset: Color32::from_rgba_premultiplied(11, 11, 28, 28),
    tier_c: Color32::from_rgb(0x8a, 0x8f, 0xb8),
};

/// The same look at night: charcoal surfaces with just a hint of blue, and a softened,
/// grayed periwinkle for text, so it's easy on the eyes rather than neon.
const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x1f, 0x20, 0x27),
    content_bg: Color32::from_rgb(0x16, 0x17, 0x1c),
    content_bg_hover: Color32::from_rgb(0x2c, 0x2d, 0x37),
    hint: Color32::from_rgb(0x5a, 0x5c, 0x70),
    primary: Color32::from_rgb(0xa4, 0xa6, 0xd4),
    border: Color32::from_rgb(0x46, 0x48, 0x5c),
    titlebar_bg: Color32::from_rgb(0x25, 0x26, 0x2e),
    titlebar_text: Color32::from_rgb(0xb3, 0xb4, 0xd2),
    titlebar_line: Color32::from_rgb(0x46, 0x48, 0x5c),
    accent: Color32::from_rgb(0x6c, 0xa6, 0xc8),
    dim: Color32::from_rgb(0x8d, 0x8f, 0xab),
    loss: Color32::from_rgb(0xe8, 0x68, 0x80),
    gain: Color32::from_rgb(0x4f, 0xbf, 0x86),
    stripe: Color32::from_rgb(0x25, 0x26, 0x2e),
    grid: Color32::from_rgb(0x37, 0x38, 0x45),
    input_bg: Color32::from_rgb(0x19, 0x1a, 0x20),
    input_border: Color32::from_rgb(0x4e, 0x50, 0x64),
    input_inset: Color32::from_rgba_premultiplied(0, 0, 0, 70),
    tier_c: Color32::from_rgb(0x6f, 0x71, 0x84),
};

/// Whether the dark palette is in use. Set once per frame by `sync`, before anything is
/// drawn, so a whole frame is always one palette.
static IS_DARK: AtomicBool = AtomicBool::new(false);

fn palette() -> &'static Palette {
    if IS_DARK.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

/// Switches the palette to match egui's current theme (the user's choice, or the system's
/// when following it). Call at the start of every frame.
pub fn sync(ctx: &egui::Context) {
    IS_DARK.store(ctx.theme() == egui::Theme::Dark, Ordering::Relaxed);
}

// One function per color, named after its palette field: `theme::primary()`.
macro_rules! colors {
    ($($name:ident),* $(,)?) => {
        $(
            pub fn $name() -> Color32 {
                palette().$name
            }
        )*
    };
}

colors!(
    bg,
    content_bg,
    content_bg_hover,
    hint,
    primary,
    border,
    titlebar_bg,
    titlebar_text,
    titlebar_line,
    accent,
    dim,
    loss,
    gain,
    stripe,
    grid,
    input_bg,
    input_border,
);

pub const PAD: f32 = 5.0;
pub const BORDER: f32 = 2.0;
pub const MARGIN: f32 = 10.0;
/// Hard drop shadow offset (no blur).
pub const SHADOW: f32 = 4.0;

const BOLD: &str = "bold";

pub fn bold() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

/// Hover and selection fades: quick enough to feel instant, slow enough to see.
const FAST: f32 = 0.08;

/// Shadow offset under a button: 3 at rest, 2 hovered, 1 pressed, eased between them so the
/// button sinks in and springs back.
fn press_offset(ui: &egui::Ui, resp: &Response) -> f32 {
    let target = if resp.is_pointer_button_down_on() {
        1.0
    } else if resp.hovered() {
        2.0
    } else {
        3.0
    };
    ui.ctx()
        .animate_value_with_time(resp.id.with("press"), target, 0.05)
}

/// 0 -> 1 as the pointer moves onto the widget.
fn hover_t(ui: &egui::Ui, resp: &Response) -> f32 {
    ui.ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), FAST)
}

/// 0 -> 1 as the widget becomes the selected one.
fn selected_t(ui: &egui::Ui, resp: &Response, selected: bool) -> f32 {
    ui.ctx()
        .animate_bool_with_time(resp.id.with("selected"), selected, FAST)
}

/// How opaque the hard drop shadows are.
const SHADOW_OPACITY: f32 = 0.6;

/// `color` as a drop shadow: see-through, so it reads as depth rather than a second border.
pub fn shadow_color(color: Color32) -> Color32 {
    color.gamma_multiply(SHADOW_OPACITY)
}

/// Unblurred drop shadow `offset` down and right, in `color` at shadow opacity.
pub fn hard_shadow(offset: f32, color: Color32) -> Shadow {
    Shadow {
        offset: Vec2::splat(offset),
        blur: 0.0,
        spread: 0.0,
        color: shadow_color(color),
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

    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        IS_DARK.store(theme == egui::Theme::Dark, Ordering::Relaxed);
        ctx.set_style_of(theme, style_for(theme));
    }
}

/// The app's look (fonts sizes, spacing, colors) in the light or dark palette; `palette()`
/// must already be set to match.
fn style_for(theme: egui::Theme) -> egui::Style {
    let mut style = egui::Style {
        visuals: theme.default_visuals(),
        ..Default::default()
    };
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
    v.override_text_color = Some(primary());
    v.panel_fill = bg();
    v.window_fill = bg();
    v.extreme_bg_color = bg();
    v.faint_bg_color = stripe();
    v.code_bg_color = stripe();
    v.hyperlink_color = primary();
    v.warn_fg_color = loss();
    v.error_fg_color = loss();
    v.selection.bg_fill = accent().gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(BORDER, primary());
    v.text_cursor.stroke = Stroke::new(BORDER, primary());
    v.window_rounding = Rounding::ZERO;
    v.menu_rounding = Rounding::ZERO;
    v.window_shadow = hard_shadow(6.0, border());
    v.popup_shadow = hard_shadow(SHADOW, border());
    v.window_stroke = Stroke::new(BORDER, border());

    let w = &mut v.widgets;
    for (state, fill) in [
        (&mut w.noninteractive, bg()),
        (&mut w.inactive, bg()),
        (&mut w.hovered, stripe()),
        (&mut w.active, accent()),
        (&mut w.open, stripe()),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(BORDER, border());
        state.fg_stroke = Stroke::new(1.5_f32, primary());
        state.rounding = Rounding::ZERO;
        state.expansion = 0.0;
    }
    style
}

/// Draws a widget with a hard shadow behind it. The shadow shrinks on hover/press so
/// buttons feel like they push in.
pub fn shadowed(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> Response) -> Response {
    shadowed_in(ui, border(), add)
}

/// `shadowed` with the shadow in `color` (e.g. red for a destructive button).
pub fn shadowed_in(
    ui: &mut egui::Ui,
    color: Color32,
    add: impl FnOnce(&mut egui::Ui) -> Response,
) -> Response {
    let slot = ui.painter().add(Shape::Noop);
    let resp = add(ui);
    let offset = press_offset(ui, &resp);
    ui.painter().set(
        slot,
        Shape::rect_filled(
            resp.rect.translate(Vec2::splat(offset)),
            0.0,
            shadow_color(color),
        ),
    );
    resp
}

/// `color` at 60% brightness (e.g. deep red under a red button).
pub fn darker(color: Color32) -> Color32 {
    let f = |c: u8| (c as f32 * 0.6) as u8;
    Color32::from_rgb(f(color.r()), f(color.g()), f(color.b()))
}

/// Button whose border, shadow and text all use `color`; `filled` makes it solid `color`
/// with white text. Used for destructive (red) and call-to-action buttons so the button
/// is one color instead of colored text in a periwinkle box.
pub fn colored_button(ui: &mut egui::Ui, text: &str, color: Color32, filled: bool) -> Response {
    let (fill, text_color) = if filled { (color, bg()) } else { (bg(), color) };
    let button = egui::Button::new(egui::RichText::new(text).color(text_color))
        .fill(fill)
        .stroke(Stroke::new(BORDER, color));
    // A solid button's shadow in its own color just reads as a thicker button; a darker
    // shade makes it look raised like the outlined ones.
    let shadow = if filled { darker(color) } else { color };
    shadowed_in(ui, shadow, |ui| ui.add(button)).on_hover_cursor(egui::CursorIcon::PointingHand)
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
        .fill(bg())
        .stroke(Stroke::new(BORDER, color))
        .shadow(hard_shadow(SHADOW, color))
        .inner_margin(egui::Margin::same(MARGIN))
}

/// A primary-colored `panel` that fills the available width exactly (leaving room for its shadow).
pub fn boxed<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    boxed_colored(ui, border(), add)
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
    p.rect_filled(rect, 0.0, accent());
    let seg = Rect::from_min_size(
        egui::pos2(rect.left() + segment_x, rect.top()),
        Vec2::new(segment_w, rect.height()),
    );
    p.rect_filled(seg, 0.0, primary());
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
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .family(bold())
                .size(14.0)
                .color(primary()),
        );
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), BORDER), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, border());
    });
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
    // A tinted block behind the selected tab (sliding with the underline), and a fainter one
    // behind a hovered tab, so the current tab stands out even where the text colors are close.
    let block = |left: f32| {
        Rect::from_min_size(
            egui::pos2(rect.left() + left, rect.top() - 4.0),
            Vec2::new(seg_w, rect.height() - PAD + 4.0),
        )
    };
    if let Some(pos) = resp.hover_pos() {
        let hovered = (((pos.x - rect.left()) / seg_w) as usize).min(labels.len() - 1);
        if hovered != *selected {
            ui.painter().rect_filled(
                block(hovered as f32 * seg_w),
                0.0,
                primary().gamma_multiply(0.07),
            );
        }
    }
    ui.painter()
        .rect_filled(block(x), 0.0, primary().gamma_multiply(0.16));
    for (i, label) in labels.iter().enumerate() {
        let color = if i == *selected { primary() } else { dim() };
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
    daily: Option<bool>,
) -> Response {
    let font = FontId::new(15.0, bold());
    // Leave room on the right for the shadow so it isn't clipped by the panel.
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - 4.0, font.size + 16.0),
        Sense::click(),
    );
    let offset = press_offset(ui, &resp);
    let sel = selected_t(ui, &resp, selected);
    let fill = bg()
        .lerp_to_gamma(stripe(), hover_t(ui, &resp))
        .lerp_to_gamma(primary(), sel);
    let text_color = primary().lerp_to_gamma(bg(), sel);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, fill);
    p.rect_stroke(
        rect.shrink(BORDER / 2.0),
        0.0,
        Stroke::new(BORDER, border()),
    );

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
    let mut right = arrow_x - 12.0;
    if let Some((s, c)) = trailing {
        let small = TextStyle::Small.resolve(ui.style());
        let color = c.lerp_to_gamma(bg(), sel);
        let r = p.text(
            egui::pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            s,
            small,
            color,
        );
        right = r.left() - 8.0;
    }
    // Daily marker: a hollow square until today's daily is logged, then a filled one that
    // pops in from the middle.
    if let Some(claimed) = daily {
        let center = egui::pos2(right - 4.0, rect.center().y);
        let square = Rect::from_center_size(center, Vec2::splat(9.0));
        let fill = ui
            .ctx()
            .animate_bool_with_time(resp.id.with("daily"), claimed, 0.15);
        let color = primary()
            .lerp_to_gamma(gain(), fill)
            .lerp_to_gamma(bg(), sel);
        if fill < 1.0 {
            p.rect_stroke(square.shrink(0.75), 0.0, Stroke::new(1.5_f32, color));
        }
        if fill > 0.0 {
            // Overshoots a little before settling, like a stamp.
            let scale = fill * (1.0 + 0.3 * (fill * std::f32::consts::PI).sin());
            p.rect_filled(
                Rect::from_center_size(center, Vec2::splat(9.0 * scale)),
                0.0,
                color,
            );
        }
    }
    let tooltip = match daily {
        Some(true) => "Daily claimed today",
        Some(false) => "Daily not claimed yet today",
        None => "",
    };
    let resp = if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    };
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sidebar card showing the open project ("PROJECT" caption over its name) with a ▾ arrow;
/// clicking it opens the project menu.
pub fn project_button(ui: &mut egui::Ui, name: &str) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width() - 4.0, 46.0), Sense::click());
    let offset = press_offset(ui, &resp);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, bg().lerp_to_gamma(stripe(), hover_t(ui, &resp)));
    p.rect_stroke(
        rect.shrink(BORDER / 2.0),
        0.0,
        Stroke::new(BORDER, border()),
    );
    let text_area = Rect::from_min_max(rect.min, egui::pos2(rect.right() - 26.0, rect.bottom()));
    let p = p.with_clip_rect(text_area);
    p.text(
        rect.left_top() + Vec2::new(10.0, 7.0),
        Align2::LEFT_TOP,
        "PROJECT",
        FontId::new(10.5, bold()),
        dim(),
    );
    p.text(
        rect.left_bottom() + Vec2::new(10.0, -7.0),
        Align2::LEFT_BOTTOM,
        name,
        FontId::new(15.0, bold()),
        primary(),
    );
    // ▾ arrow
    let (cx, cy, h) = (rect.right() - 14.0, rect.center().y, 5.0);
    ui.painter().add(Shape::convex_polygon(
        vec![
            egui::pos2(cx - h, cy - h * 0.6),
            egui::pos2(cx + h, cy - h * 0.6),
            egui::pos2(cx, cy + h * 0.6),
        ],
        primary(),
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
pub fn select_button(ui: &mut egui::Ui, text: &str, width: f32, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let offset = press_offset(ui, &resp);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, bg().lerp_to_gamma(stripe(), hover_t(ui, &resp)));
    p.rect_stroke(
        rect.shrink(BORDER / 2.0),
        0.0,
        Stroke::new(BORDER, border()),
    );
    // Text runs up to just before the arrow.
    let clip = Rect::from_min_max(rect.min, egui::pos2(rect.right() - 18.0, rect.bottom()));
    p.with_clip_rect(clip).text(
        rect.left_center() + Vec2::new(8.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::new(14.0, bold()),
        primary(),
    );
    let (cx, cy, h) = (rect.right() - 10.0, rect.center().y, 4.0);
    p.add(Shape::convex_polygon(
        vec![
            egui::pos2(cx - h, cy - h * 0.6),
            egui::pos2(cx + h, cy - h * 0.6),
            egui::pos2(cx, cy + h * 0.6),
        ],
        primary(),
        Stroke::NONE,
    ));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square button with an X drawn as two lines (the ✕ character isn't in the font and
/// renders as an empty box). `color` is the X; `frame` the border and hard shadow.
pub fn x_button(ui: &mut egui::Ui, color: Color32, frame: Color32, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    paint_x_button(ui, rect, resp, color, frame)
}

/// `x_button` at a given spot, on top of whatever is there (e.g. inside a clickable row).
pub fn x_button_at(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    color: Color32,
    frame: Color32,
) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    paint_x_button(ui, rect, resp, color, frame)
}

fn paint_x_button(
    ui: &mut egui::Ui,
    rect: Rect,
    resp: Response,
    color: Color32,
    frame: Color32,
) -> Response {
    let size = rect.width();
    let offset = press_offset(ui, &resp);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(frame),
    );
    p.rect_filled(rect, 0.0, bg().lerp_to_gamma(stripe(), hover_t(ui, &resp)));
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, frame));
    let c = rect.center();
    let r = size * 0.18;
    let stroke = Stroke::new(2.0_f32, color);
    p.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
    p.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square shadowed button with a nav icon and an optional count beside it (e.g. the
/// "Add casino" list's hidden-casinos tab). Filled primary while `selected`.
pub fn icon_button(ui: &mut egui::Ui, icon: NavIcon, count: usize, selected: bool) -> Response {
    let font = FontId::new(13.0, bold());
    let label = (count > 0).then(|| count.to_string());
    let text_w = label.as_ref().map_or(0.0, |t| {
        ui.painter()
            .layout_no_wrap(t.clone(), font.clone(), primary())
            .size()
            .x
            + 6.0
    });
    let size = Vec2::new(30.0 + text_w, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let offset = press_offset(ui, &resp);
    let sel = selected_t(ui, &resp, selected);
    let fill = bg()
        .lerp_to_gamma(stripe(), hover_t(ui, &resp))
        .lerp_to_gamma(primary(), sel);
    let fg = primary().lerp_to_gamma(bg(), sel);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, fill);
    p.rect_stroke(
        rect.shrink(BORDER / 2.0),
        0.0,
        Stroke::new(BORDER, border()),
    );
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 15.0, rect.center().y),
        Vec2::splat(14.0),
    );
    paint_nav_icon(p, icon, icon_rect, fg);
    if let Some(t) = label {
        p.text(
            egui::pos2(icon_rect.right() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            t,
            font,
            fg,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Option row in a dropdown: the current choice is filled primary with white text (like
/// the selected sidebar row); others get a clear tint on hover.
pub fn menu_option(ui: &mut egui::Ui, label: &str, selected: bool) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    let text_color = if selected {
        p.rect_filled(rect, 0.0, primary());
        bg()
    } else {
        if resp.hovered() {
            p.rect_filled(rect, 0.0, content_bg_hover());
        }
        primary()
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
        p.rect_filled(rect, 0.0, content_bg_hover());
    }
    p.text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.5, bold()),
        primary(),
    );
    if !shortcut.is_empty() {
        p.text(
            rect.right_center() - Vec2::new(10.0, 0.0),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::new(12.5, bold()),
            dim(),
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
    let (hover, sel) = (hover_t(ui, &resp), selected_t(ui, &resp, selected));
    let p = ui.painter();
    // Faded in over the transparent sidebar (gamma_multiply scales alpha too).
    p.rect_filled(rect, 0.0, content_bg_hover().gamma_multiply(hover));
    p.rect_filled(rect, 0.0, primary().gamma_multiply(sel));
    let color = primary().lerp_to_gamma(bg(), sel);
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
        let galley = p.layout_no_wrap(t, font, if selected { primary() } else { bg() });
        let badge = Rect::from_center_size(
            rect.right_center() - Vec2::new(10.0 + galley.size().x / 2.0 + 6.0, 0.0),
            galley.size() + Vec2::new(12.0, 4.0),
        );
        p.rect_filled(badge, 0.0, if selected { bg() } else { primary() });
        p.galley(badge.center() - galley.size() / 2.0, galley, primary());
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Casino logo (loaded from the web and cached by egui) inside a 1px primary border.
pub fn logo_at(ui: &egui::Ui, url: &str, rect: Rect) {
    egui::Image::new(url).paint_at(ui, rect);
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, border()));
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
        "S" => gain(),
        "A" => primary(),
        "B" => dim(),
        _ => palette().tier_c,
    };
    p.rect_filled(
        rect.translate(Vec2::splat(2.0)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, fill);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        tier,
        FontId::new(rect.height() * 0.6, bold()),
        bg(),
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

/// Compact button for table rows (e.g. "Mark received"): small bold text in `color`, a 2px
/// border and a hard shadow that presses in, like the full-size buttons.
pub fn small_button(ui: &mut egui::Ui, text: &str, color: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::new(12.0, bold()), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(14.0, 6.0), Sense::click());
    // Two-thirds of the full-size press, to suit the smaller button.
    let offset = press_offset(ui, &resp) * 2.0 / 3.0;
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(offset)),
        0.0,
        shadow_color(color),
    );
    p.rect_filled(rect, 0.0, bg().lerp_to_gamma(stripe(), hover_t(ui, &resp)));
    p.rect_stroke(rect.shrink(BORDER / 2.0), 0.0, Stroke::new(BORDER, color));
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    // Room for the shadow before whatever comes next.
    ui.add_space(2.0);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Hover breakdown: labels on the left, amounts right-aligned, and an optional total under a
/// rule. Padded itself, since popups share the dropdowns' thin margin.
pub fn breakdown_tooltip(
    ui: &mut egui::Ui,
    rows: &[(String, String)],
    total: Option<(&str, String)>,
) {
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let row = |ui: &mut egui::Ui, label: &str, value: &str, strong: bool| {
                let font = if strong {
                    bold()
                } else {
                    egui::FontFamily::Proportional
                };
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .family(font.clone())
                        .color(if strong { primary() } else { dim() }),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(value).family(font).color(primary()));
                });
                ui.end_row();
            };
            egui::Grid::new("breakdown")
                .num_columns(2)
                .min_col_width(70.0)
                .spacing([24.0, 4.0])
                .show(ui, |ui| {
                    for (label, value) in rows {
                        row(ui, label, value, false);
                    }
                });
            if let Some((label, value)) = total {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.min_rect().width(), 1.0), Sense::hover());
                ui.painter().rect_filled(rect, 0.0, grid());
                egui::Grid::new("breakdown_total")
                    .num_columns(2)
                    .min_col_width(70.0)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        row(ui, label, &value, true);
                    });
            }
        });
}

/// Like `boxed`, but with no inner padding: content runs right up to the 2px border.
pub fn boxed_flush<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner_width = (ui.available_width() - 2.0 * BORDER - SHADOW).max(0.0);
    panel(border())
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
    let stroke = Stroke::new(1.0_f32, grid());
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
                .color(dim()),
        );
        line(
            ui,
            egui::RichText::new(value)
                .size(15.0)
                .family(bold())
                .color(primary()),
        );
    })
    .response;
    if detail.is_empty() {
        resp.on_hover_text(value)
    } else {
        resp.on_hover_text(format!("{value}\n{detail}"))
    }
}

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
    let inset = palette().input_inset;
    let border = if focused {
        Stroke::new(BORDER, primary())
    } else if ui.rect_contains_pointer(r) {
        Stroke::new(1.5_f32, primary())
    } else {
        Stroke::new(1.5_f32, input_border())
    };
    let mut shapes = Vec::new();
    if focused {
        shapes.push(Shape::rect_filled(
            r.expand(3.0),
            0.0,
            accent().gamma_multiply(0.45),
        ));
    }
    shapes.push(Shape::rect_filled(r, 0.0, input_bg()));
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
    let button = egui::Button::new(egui::RichText::new(text).color(bg()))
        .fill(primary())
        .min_size(Vec2::new(width, control_height(ui)));
    let resp = shadowed_in(ui, darker(primary()), |ui| ui.add_enabled(enabled, button));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Progress bar: bordered accent track with a primary fill and a hard shadow.
pub fn progress(ui: &mut egui::Ui, fraction: f32) {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SHADOW, 14.0),
        Sense::hover(),
    );
    // Slides to a new value (switching casinos, adding an entry) instead of jumping.
    let fraction =
        ui.ctx()
            .animate_value_with_time(resp.id.with("fill"), fraction.clamp(0.0, 1.0), 0.25);
    let p = ui.painter();
    p.rect_filled(
        rect.translate(Vec2::splat(3.0)),
        0.0,
        shadow_color(border()),
    );
    p.rect_filled(rect, 0.0, accent());
    let fill = Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    p.rect_filled(fill, 0.0, primary());
    p.rect_stroke(
        rect.shrink(BORDER / 2.0),
        0.0,
        Stroke::new(BORDER, border()),
    );
}
