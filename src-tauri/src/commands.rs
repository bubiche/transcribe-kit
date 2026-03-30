use tauri::State;

use crate::{
    models::{ApiModelDescriptor, AppSettings, LocalModelDescriptor, SaveSettingsRequest},
    providers::{api_openai_compatible, local_whisper},
    settings::SettingsStore,
};

const LOCAL_MODEL_IDS: &[&str] = &[
    "whisper-tiny",
    "whisper-base",
    "whisper-small",
    "whisper-large-v3-turbo",
];
const API_MODEL_IDS: &[&str] = &["gpt-4o-mini-transcribe", "gpt-4o-transcribe", "custom"];

#[tauri::command]
pub fn health_check() -> String {
    "ok".to_string()
}

#[tauri::command]
pub fn list_local_models() -> Vec<LocalModelDescriptor> {
    vec![
        LocalModelDescriptor {
            id: "whisper-tiny".to_string(),
            label: "Whisper Tiny".to_string(),
            engine: local_whisper::ENGINE_ID.to_string(),
        },
        LocalModelDescriptor {
            id: "whisper-base".to_string(),
            label: "Whisper Base".to_string(),
            engine: local_whisper::ENGINE_ID.to_string(),
        },
        LocalModelDescriptor {
            id: "whisper-small".to_string(),
            label: "Whisper Small".to_string(),
            engine: local_whisper::ENGINE_ID.to_string(),
        },
        LocalModelDescriptor {
            id: "whisper-large-v3-turbo".to_string(),
            label: "Whisper Large v3 Turbo".to_string(),
            engine: local_whisper::ENGINE_ID.to_string(),
        },
    ]
}

#[tauri::command]
pub fn list_api_models() -> Vec<ApiModelDescriptor> {
    vec![
        ApiModelDescriptor {
            id: "gpt-4o-mini-transcribe".to_string(),
            label: "GPT-4o mini Transcribe".to_string(),
            provider: api_openai_compatible::PROVIDER_ID.to_string(),
            supports_custom_name: false,
        },
        ApiModelDescriptor {
            id: "gpt-4o-transcribe".to_string(),
            label: "GPT-4o Transcribe".to_string(),
            provider: api_openai_compatible::PROVIDER_ID.to_string(),
            supports_custom_name: false,
        },
        ApiModelDescriptor {
            id: "custom".to_string(),
            label: "Custom model name".to_string(),
            provider: api_openai_compatible::PROVIDER_ID.to_string(),
            supports_custom_name: true,
        },
    ]
}

#[tauri::command]
pub fn get_settings(store: State<'_, SettingsStore>) -> Result<AppSettings, String> {
    store.load().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_settings(
    request: SaveSettingsRequest,
    store: State<'_, SettingsStore>,
) -> Result<AppSettings, String> {
    store
        .save(request, LOCAL_MODEL_IDS, API_MODEL_IDS)
        .map_err(|error| error.to_string())
}
