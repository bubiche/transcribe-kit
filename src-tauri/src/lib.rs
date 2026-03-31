mod audio;
mod commands;
mod models;
mod providers;
mod settings;

use commands::LocalEngineState;
use settings::SettingsStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings_store = SettingsStore::new().expect("settings store");

    tauri::Builder::default()
        .manage(settings_store)
        .manage(LocalEngineState::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::get_settings,
            commands::list_local_models,
            commands::list_api_models,
            commands::save_settings,
            commands::get_model_status,
            commands::delete_model,
            commands::ensure_model_downloaded,
            commands::start_file_transcription
        ])
        .setup(|app| {
            let main_window = app.get_webview_window("main").expect("main window");
            main_window.set_title("Transcribe Kit")?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
