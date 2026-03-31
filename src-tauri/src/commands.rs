use std::path::PathBuf;
use std::sync::Mutex;

use tauri::ipc::Channel;
use tauri::State;

use crate::{
    audio,
    models::{
        ApiModelDescriptor, AppSettings, LocalModelDescriptor, ModelDownloadProgress, ModelStatus,
        SaveSettingsRequest, TranscriptResult,
    },
    providers::{local_whisper, local_whisper::WhisperEngine, TranscribeLocal},
    settings::SettingsStore,
};

const LOCAL_MODEL_IDS: &[&str] = &[
    "whisper-tiny",
    "whisper-base",
    "whisper-small",
    "whisper-large-v3-turbo",
];
const API_MODEL_IDS: &[&str] = &["gpt-4o-mini-transcribe", "gpt-4o-transcribe", "custom"];

pub struct LocalEngineState {
    pub inner: Mutex<Option<WhisperEngine>>,
}

impl LocalEngineState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn health_check() -> String {
    "ok".to_string()
}

#[tauri::command]
pub fn list_local_models() -> Vec<LocalModelDescriptor> {
    LOCAL_MODEL_IDS
        .iter()
        .map(|id| {
            let downloaded = local_whisper::expected_model_path(id)
                .map(|p| p.exists())
                .unwrap_or(false);

            LocalModelDescriptor {
                id: id.to_string(),
                label: whisper_label(id),
                engine: local_whisper::ENGINE_ID.to_string(),
                downloaded,
                size_label: local_whisper::size_label(id).to_string(),
            }
        })
        .collect()
}

fn whisper_label(model_id: &str) -> String {
    match model_id {
        "whisper-tiny" => "Whisper Tiny",
        "whisper-base" => "Whisper Base",
        "whisper-small" => "Whisper Small",
        "whisper-large-v3-turbo" => "Whisper Large v3 Turbo",
        _ => model_id,
    }
    .to_string()
}

#[tauri::command]
pub fn list_api_models() -> Vec<ApiModelDescriptor> {
    vec![
        ApiModelDescriptor {
            id: "gpt-4o-mini-transcribe".to_string(),
            label: "GPT-4o mini Transcribe".to_string(),
            provider: crate::providers::api_openai_compatible::PROVIDER_ID.to_string(),
            supports_custom_name: false,
        },
        ApiModelDescriptor {
            id: "gpt-4o-transcribe".to_string(),
            label: "GPT-4o Transcribe".to_string(),
            provider: crate::providers::api_openai_compatible::PROVIDER_ID.to_string(),
            supports_custom_name: false,
        },
        ApiModelDescriptor {
            id: "custom".to_string(),
            label: "Custom model name".to_string(),
            provider: crate::providers::api_openai_compatible::PROVIDER_ID.to_string(),
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

#[tauri::command]
pub fn get_model_status(model_id: String) -> Result<ModelStatus, String> {
    local_whisper::model_status(&model_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_model(model_id: String) -> Result<(), String> {
    local_whisper::delete_model(&model_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ensure_model_downloaded(
    model_id: String,
    on_progress: Channel<ModelDownloadProgress>,
) -> Result<(), String> {
    local_whisper::download_model(&model_id, &on_progress)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_file_transcription(
    file_path: String,
    engine_state: State<'_, LocalEngineState>,
    settings_store: State<'_, SettingsStore>,
) -> Result<TranscriptResult, String> {
    let settings = settings_store.load().map_err(|e| e.to_string())?;
    let model_id = settings.local_model_id;

    let engine = get_or_load_engine(&engine_state, &model_id)?;

    let result = tokio::task::spawn_blocking(move || -> Result<TranscriptResult, String> {
        let samples = audio::decode_wav_file(&PathBuf::from(&file_path))
            .map_err(|e| e.to_string())?;
        engine.transcribe_pcm(&samples).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))??;

    Ok(result)
}

fn get_or_load_engine(
    state: &LocalEngineState,
    model_id: &str,
) -> Result<WhisperEngine, String> {
    {
        let guard = state.inner.lock().unwrap();
        if let Some(ref engine) = *guard {
            if engine.model_id() == model_id {
                return Ok(engine.clone());
            }
        }
    }

    let model_path = local_whisper::resolve_model_path(model_id).map_err(|e| e.to_string())?;
    let path_str = model_path
        .to_str()
        .ok_or("Model path contains invalid UTF-8")?;

    let engine = WhisperEngine::load(path_str, model_id.to_string())
        .map_err(|e| e.to_string())?;

    let mut guard = state.inner.lock().unwrap();
    *guard = Some(engine.clone());

    Ok(engine)
}
