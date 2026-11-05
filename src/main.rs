// No console window behind the app on Windows (`--mcp` still talks over the pipes it is given).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod compositor;
mod export;
mod gpu;
mod hwenc;
mod library;
mod mcp;
mod media;
mod model;
mod model3d;
mod platform;
mod post;
mod text;
mod thumbs;
mod theme;
mod timeline;
mod update;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--mcp") {
        mcp::run(std::env::args().any(|a| a == "--resumed"));
        return Ok(());
    }
    update::cleanup();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rec·ording")
            // Matches rec-ording.desktop, so Wayland docks find the icon.
            .with_app_id("rec-ording")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/rec-ording-256.png")).unwrap_or_default())
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1080.0, 680.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "rec-ording",
        native_options,
        Box::new(|cc| Ok(Box::new(app::RecOrdingApp::new(cc)))),
    )
}
