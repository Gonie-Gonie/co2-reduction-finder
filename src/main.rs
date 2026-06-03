#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod domain;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([980.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "CO2 Reduction Finder",
        native_options,
        Box::new(|cc| Ok(Box::new(app::Co2App::new(cc)))),
    )
}

