mod app;
mod catalog;
mod fonts;
mod opkind;
mod params;
mod state;

use app::GuiApp;

fn main() -> eframe::Result<()> {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
    // Open at 700px wide so the port field isn't clipped. `persist_window: false`
    // keeps this size authoritative every launch (otherwise the persistence feature
    // would restore a previously-saved narrower window and override it). Input state
    // is persisted separately via `App::save`, so this does not affect 前回状態復元.
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([700.0, 600.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "vstc_gui",
        native_options,
        Box::new(move |cc| Ok(Box::new(GuiApp::new(cc, runtime)))),
    )
}
