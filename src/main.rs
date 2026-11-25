// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod grail;
mod logos;
mod model;
mod store;
#[cfg(test)]
mod stress;
mod tax;
mod theme;
mod titlebar;
mod update;

use eframe::egui;

/// Sent with every web request (SweepsGrail, casino logos, GitHub releases).
pub const USER_AGENT: &str = concat!(
    "SweepsTracker/",
    env!("CARGO_PKG_VERSION"),
    " (personal spending tracker)"
);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: titlebar::configure(
            egui::ViewportBuilder::default()
                .with_title("Sweeps Tracker")
                // Linux desktops match the window to its .desktop entry and icon by this
                // (the Flatpak's app id).
                .with_app_id("net.packboy.sweepstracker")
                .with_inner_size([1150.0, 720.0])
                // Wide enough for the widest sidebar (280) plus the casino page header's buttons
                // and the entries table's columns; tall enough for the casino page.
                .with_min_inner_size([960.0, 620.0])
                // Window / Dock / taskbar icon (the .app bundle also carries AppIcon.icns).
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                        .expect("valid icon PNG"),
                ),
        ),
        ..Default::default()
    };
    eframe::run_native(
        "Sweeps Tracker",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // Added last so it's tried first for http(s) images: casino logos, cached on disk.
            logos::LogoLoader::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx)))
        }),
    )
}
