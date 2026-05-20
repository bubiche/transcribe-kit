mod audio;
mod audio_monitor;
mod commands;
mod engine;
mod hotkeys;
mod input_devices;
mod live_recording;
mod llm_engine;
mod models;
mod notes;
mod providers;
mod recording_tray;
mod settings;
mod templates;
mod transcription;

use audio_monitor::AudioMonitorState;
use engine::LocalEngineState;
use hotkeys::HotkeyManagerState;
use live_recording::LiveRecordingManagerState;
use llm_engine::{LlmServerState, PostprocessCancelState};
use notes::NoteStore;
use settings::SettingsStore;
use tauri::Manager;
use templates::TemplateStore;

fn fatal_dialog(message: &str) -> ! {
    eprintln!("Fatal: {message}");
    // Best-effort native dialog — rfd works without a Tauri app handle.
    #[cfg(not(test))]
    {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Transcribe Kit — startup error")
            .set_description(message)
            .show();
    }
    std::process::exit(1);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings_store = match SettingsStore::new() {
        Ok(store) => store,
        Err(error) => fatal_dialog(&format!(
            "Could not initialize settings: {error}\n\nTranscribe Kit will now exit."
        )),
    };
    let template_store = match TemplateStore::new() {
        Ok(store) => store,
        Err(error) => fatal_dialog(&format!(
            "Could not initialize templates: {error}\n\nTranscribe Kit will now exit."
        )),
    };
    let note_store = match NoteStore::new() {
        Ok(store) => store,
        Err(error) => fatal_dialog(&format!(
            "Could not initialize notes: {error}\n\nTranscribe Kit will now exit."
        )),
    };
    let preload_settings_store = settings_store.clone();
    let engine_state = LocalEngineState::new();
    let preload_engine_state = engine_state.clone();
    let hotkey_state = HotkeyManagerState::new();
    let preload_hotkey_state = hotkey_state.clone();
    let llm_server_state = LlmServerState::new();
    let preload_llm_server_state = llm_server_state.clone();
    let cancel_state = PostprocessCancelState::new();
    let live_recording_state = LiveRecordingManagerState::new();
    let audio_monitor_state = AudioMonitorState::new();

    tauri::Builder::default()
        .manage(settings_store)
        .manage(template_store)
        .manage(note_store)
        .manage(engine_state)
        .manage(hotkey_state)
        .manage(live_recording_state)
        .manage(llm_server_state)
        .manage(cancel_state)
        .manage(audio_monitor_state)
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
            commands::start_file_transcription,
            commands::transcribe_live_recording,
            commands::list_templates,
            commands::save_templates,
            commands::run_postprocess,
            commands::list_local_llm_models,
            commands::get_llm_model_status,
            commands::delete_llm_model,
            commands::ensure_llm_model_downloaded,
            commands::preload_local_llm_model,
            commands::cancel_postprocess,
            commands::start_audio_monitor,
            commands::stop_audio_monitor,
            commands::write_text_file,
            commands::list_notes,
            commands::get_note,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::delete_app_data
        ])
        .setup(move |app| {
            // Kill any orphaned llama-server from a previous crash before anything else
            llm_engine::cleanup_orphaned_sidecar();

            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.set_title("Transcribe Kit");
            }
            if let Err(error) = recording_tray::initialize(app.handle()) {
                eprintln!("Failed to initialize tray icon: {error}");
            }
            engine::preload_saved_local_model(
                preload_engine_state.clone(),
                preload_settings_store.clone(),
            );
            llm_engine::preload_llm_server(
                preload_llm_server_state.clone(),
                preload_settings_store.clone(),
                app.handle().clone(),
            );
            preload_hotkey_state.initialize_from_settings(app.handle(), &preload_settings_store);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
