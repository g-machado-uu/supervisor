// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ics;
mod model;
mod storage;
mod ui;

fn main() -> eframe::Result {
    // Optional: path to a data file to open instead of the last-used one.
    let data_file = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Supervisor")
            .with_app_id("supervisor")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../packaging/supervisor.png"))
                    .expect("bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "Supervisor",
        options,
        Box::new(|cc| Ok(Box::new(app::SupervisorApp::new(cc, data_file)))),
    )
}
