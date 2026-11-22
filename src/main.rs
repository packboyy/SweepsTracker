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

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: titlebar::configure(
            egui::ViewportBuilder::default()
                .with_title("Sweeps Tracker")
                .with_inner_size([1150.0, 720.0])
                .with_min_inner_size([820.0, 520.0]),
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
