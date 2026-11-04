mod app;
mod capture;
mod compositor;
mod export;
mod library;
mod mcp;
mod media;
mod model;
mod model3d;
mod theme;
mod timeline;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--mcp") {
        mcp::run();
        return Ok(());
    }
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rec-ording")
            .with_app_id("rec-ording")
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
