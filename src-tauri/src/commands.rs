use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::State;

use crate::{
    audio, hotkeys, input_devices,
    models::{
        ApiModelDescriptor, AppSettings, AudioInputDeviceDescriptor, InputType,
        LocalModelDescriptor, ModelDownloadProgress, ModelStatus, SaveSettingsRequest,
        StartFileTranscriptionRequest, TranscriptResult, TranscriptionStreamEvent,
    },
    providers::{local_whisper, local_whisper::WhisperEngine},
    settings::SettingsStore,
};

const LOCAL_MODEL_IDS: &[&str] = &[
    "whisper-tiny",
    "whisper-base",
    "whisper-small",
    "whisper-large-v3-turbo",
];
const API_MODEL_IDS: &[&str] = &["gpt-4o-mini-transcribe", "gpt-4o-transcribe", "custom"];

#[derive(Clone)]
pub struct LocalEngineState {
    pub inner: Arc<Mutex<Option<WhisperEngine>>>,
}

impl LocalEngineState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
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
pub fn list_input_devices() -> Result<Vec<AudioInputDeviceDescriptor>, String> {
    input_devices::list_input_devices().map_err(|error| error.to_string())
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
pub fn get_settings(
    store: State<'_, SettingsStore>,
    hotkey_state: State<'_, hotkeys::HotkeyManagerState>,
) -> Result<AppSettings, String> {
    let mut settings = store.load().map_err(|error| error.to_string())?;
    settings.hotkey_registration_error = hotkey_state.registration_error();
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(
    request: SaveSettingsRequest,
    app: tauri::AppHandle,
    store: State<'_, SettingsStore>,
    hotkey_state: State<'_, hotkeys::HotkeyManagerState>,
) -> Result<AppSettings, String> {
    let previous_settings = store.load().ok();
    let input_device_ids = if request
        .selected_input_device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        input_devices::list_input_devices()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|device| device.id)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let prepared = store
        .prepare_save(request, LOCAL_MODEL_IDS, API_MODEL_IDS, &input_device_ids)
        .map_err(|error| error.to_string())?;

    hotkey_state
        .apply(&app, prepared.hotkey_shortcut(), prepared.hotkey_mode())
        .map_err(|error| error.to_string())?;

    let mut settings = match store.commit_save(prepared) {
        Ok(settings) => settings,
        Err(error) => {
            if let Some(previous_settings) = previous_settings {
                if let Err(rollback_error) = hotkey_state.apply(
                    &app,
                    &previous_settings.hotkey_shortcut,
                    previous_settings.hotkey_mode,
                ) {
                    return Err(format!(
                        "{error}. Transcribe Kit also failed to restore the previous global hotkey: {rollback_error}"
                    ));
                }
            }
            return Err(error.to_string());
        }
    };
    settings.hotkey_registration_error = hotkey_state.registration_error();
    Ok(settings)
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
    request: StartFileTranscriptionRequest,
    on_update: Channel<TranscriptionStreamEvent>,
    engine_state: State<'_, LocalEngineState>,
    settings_store: State<'_, SettingsStore>,
) -> Result<TranscriptResult, String> {
    let settings = settings_store.load().map_err(|e| e.to_string())?;

    if settings.provider_mode != crate::models::ProviderMode::Local {
        return Err(
            "File import transcription is only wired up for Local Whisper right now. Switch the provider in Settings to continue.".to_string(),
        );
    }

    let model_id = settings.local_model_id;
    let file_path = PathBuf::from(&request.file_path);
    let source_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string());
    let on_update = Arc::new(on_update);
    let engine_cache = Arc::clone(&engine_state.inner);

    let result = tokio::task::spawn_blocking(move || -> Result<TranscriptResult, String> {
        let engine = get_or_load_engine(&engine_cache, &model_id)?;
        let decoded_audio = audio::decode_audio_file(&file_path).map_err(|e| e.to_string())?;
        let progress_updates = Arc::clone(&on_update);
        let segment_updates = Arc::clone(&on_update);
        let mut result = engine
            .transcribe_pcm_streaming(
                &decoded_audio.samples,
                Some(move |progress_percent| {
                    let _ = progress_updates
                        .send(TranscriptionStreamEvent::Progress { progress_percent });
                }),
                Some(move |segment_index, segment, accumulated_text| {
                    let _ = segment_updates.send(TranscriptionStreamEvent::Segment {
                        segment_index,
                        segment,
                        accumulated_text,
                    });
                }),
            )
            .map_err(|e| e.to_string())?;
        result.source.input_type = InputType::File;
        result.source.source_name = source_name;
        result.source.duration_ms = decoded_audio.duration_ms;
        Ok(result)
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))??;

    Ok(result)
}

#[tauri::command]
pub async fn preload_local_model(
    model_id: String,
    engine_state: State<'_, LocalEngineState>,
) -> Result<(), String> {
    let engine_cache = Arc::clone(&engine_state.inner);

    tokio::task::spawn_blocking(move || get_or_load_engine(&engine_cache, &model_id).map(|_| ()))
        .await
        .map_err(|e| format!("Model preload task failed: {e}"))?
}

pub fn preload_saved_local_model(engine_state: LocalEngineState, settings_store: SettingsStore) {
    std::thread::spawn(move || {
        let Ok(settings) = settings_store.load() else {
            return;
        };

        if settings.provider_mode != crate::models::ProviderMode::Local {
            return;
        }

        let _ = get_or_load_engine(&engine_state.inner, &settings.local_model_id);
    });
}

fn get_or_load_engine(
    engine_cache: &Arc<Mutex<Option<WhisperEngine>>>,
    model_id: &str,
) -> Result<WhisperEngine, String> {
    let mut guard = engine_cache.lock().unwrap();
    if let Some(ref engine) = *guard {
        if engine.model_id() == model_id {
            return Ok(engine.clone());
        }
    }

    let model_path = local_whisper::resolve_model_path(model_id).map_err(|e| e.to_string())?;
    let path_str = model_path
        .to_str()
        .ok_or("Model path contains invalid UTF-8")?;

    let engine = WhisperEngine::load(path_str, model_id.to_string()).map_err(|e| e.to_string())?;

    *guard = Some(engine.clone());

    Ok(engine)
}
