mod audio;
mod commands;
mod hotkeys;
mod input_devices;
mod live_recording;
mod models;
mod providers;
mod settings;

use commands::LocalEngineState;
use hotkeys::HotkeyManagerState;
use live_recording::LiveRecordingManagerState;
use settings::SettingsStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings_store = SettingsStore::new().expect("settings store");
    let preload_settings_store = settings_store.clone();
    let engine_state = LocalEngineState::new();
    let preload_engine_state = engine_state.clone();
    let hotkey_state = HotkeyManagerState::new();
    let preload_hotkey_state = hotkey_state.clone();
    let live_recording_state = LiveRecordingManagerState::new();

    tauri::Builder::default()
        .manage(settings_store)
        .manage(engine_state)
        .manage(hotkey_state)
        .manage(live_recording_state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::get_live_recording_status,
            commands::get_settings,
            commands::list_local_models,
            commands::list_input_devices,
            commands::list_api_models,
            commands::save_settings,
            commands::get_model_status,
            commands::delete_model,
            commands::ensure_model_downloaded,
            commands::preload_local_model,
            commands::start_live_transcription,
            commands::stop_live_transcription,
            commands::start_file_transcription
        ])
        .setup(move |app| {
            let main_window = app.get_webview_window("main").expect("main window");
            main_window.set_title("Transcribe Kit")?;
            commands::preload_saved_local_model(
                preload_engine_state.clone(),
                preload_settings_store.clone(),
            );
            preload_hotkey_state.initialize_from_settings(&app.handle(), &preload_settings_store);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
