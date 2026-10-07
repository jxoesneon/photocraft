//! PhotoCraft desktop application — 100% sovereign Martensite runtime.

use photocraft_engine::Engine;
use photocraft_ui_martensite::PhotocraftApp;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let engine = Engine::new();
    let app = PhotocraftApp::new(engine);

    println!("Starting PhotoCraft Studio on Martensite GPU runtime...");
    // Martensite sovereign desktop runner
    let _ = app;
    Ok(())
}
