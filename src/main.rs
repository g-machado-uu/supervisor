// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ics;
mod model;
#[cfg(test)]
mod perf_tests;
mod storage;
mod ui;

fn main() -> eframe::Result {
    // Optional: path to a data file to open instead of the last-used one.
    let data_file = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let mut options = eframe::NativeOptions {
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
    // On Wayland, eframe only draws when the compositor asks for a frame, so
    // also waiting for vsync when presenting adds nothing. That wait is a
    // known way for an app to hang with some drivers (NVIDIA): the window
    // stops responding until force-quit.
    if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "Supervisor",
        options,
        Box::new(|cc| Ok(Box::new(app::SupervisorApp::new(cc, data_file)))),
    )
}
