mod commands;
mod models;
mod providers;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::list_local_models,
            commands::list_api_models
        ])
        .setup(|app| {
            let main_window = app.get_webview_window("main").expect("main window");
            main_window.set_title("Transcribe Kit")?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

