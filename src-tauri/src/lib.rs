mod commands;

use redrob_core::DataService;
use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let service = DataService::persistent(app_data_dir.join("connections.json"));
            if let Some(warning) = service.profile_load_warning() {
                eprintln!("{warning}");
            }
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::profile_load_warnings,
            commands::save_connection_with_secret,
            commands::remove_connection,
            commands::test_connection,
            commands::connect,
            commands::disconnect,
            commands::load_metadata,
            commands::execute_query,
            commands::save_ai_secret,
            commands::ai_chat,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Redrob Data");
}
