//! Redrob Query desktop app: an agentic database client in one egui window.
//!
//! Usage: `redrob-query [--version]`
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod account;
mod ai_dock;
mod app;
mod changes;
mod editor;
mod grid;
mod highlight;
mod mongo;
mod profiles;
mod schema;
mod services;
mod structure;
mod theme;
mod workspace;

use app::QueryApp;

/// The 256px app icon from the brand pipeline (resources/icons, pinned in brand/SOURCES.json).
const ICON_PNG: &[u8] = include_bytes!("../../../resources/icons/hicolor/256x256/apps/redrob-query.png");

/// The main window: 1440 × 900 (eframe shrinks it to the monitor), at least 900 × 560.
fn native_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id(services::APP_ID)
        .with_title(services::APP_NAME)
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        viewport,
        centered: true,
        // eframe saves the window geometry and egui panel sizes beside the preferences.
        persistence_path: services::persistence_path(),
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--version" => {
                println!("redrob-query {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => eprintln!("redrob-query: ignoring argument {other:?}"),
        }
    }
    let mut options = native_options();
    // Nothing draws or dispatches indirectly, so wgpu's indirect-call validation only costs a
    // compute shader at start-up that some drivers cannot compile.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.flags = create
            .instance_descriptor
            .flags
            .difference(eframe::wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL)
            .with_env();
    }
    eframe::run_native(
        services::APP_NAME,
        options,
        Box::new(|cc| {
            if let Some(rs) = &cc.wgpu_render_state {
                let info = rs.adapter.get_info();
                log::info!(
                    "graphics: {} on {:?} ({:?}; driver {} {})",
                    info.name,
                    info.backend,
                    info.device_type,
                    info.driver,
                    info.driver_info
                );
            }
            Ok(Box::new(QueryApp::new(&cc.egui_ctx)))
        }),
    )
}
