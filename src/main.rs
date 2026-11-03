mod app;
mod mcp;
mod model;
mod renderer3d;
mod workers;

use app::RecOrdingApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let bin_name = args.first().map(|s| s.as_str()).unwrap_or("");

    if args.iter().any(|a| a == "--mcp") || bin_name.ends_with("rec-ording-mcp") {
        mcp::run_stdio_mcp_server();
        return Ok(());
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rec-ording — Estudio 3D Nativo en Rust + Vulkan + FFmpeg + MCP")
            .with_inner_size([1560.0, 920.0])
            .with_min_inner_size([1120.0, 680.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "rec-ording",
        native_options,
        Box::new(|cc| Ok(Box::new(RecOrdingApp::new(cc)))),
    )
}
