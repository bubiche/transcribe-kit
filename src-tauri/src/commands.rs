use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::{
    audio_monitor::AudioMonitorState,
    engine::{get_or_load_engine, LocalEngineState},
    hotkeys, input_devices, live_recording, llm_engine,
    llm_engine::{LlmServerState, PostprocessCancelState},
    models::{
        ApiModelDescriptor, AppSettings, AudioInputDeviceDescriptor, InputType, LiveCaptureProfile,
        LiveRecordingResult, LiveRecordingStatus, LocalModelDescriptor, ModelDownloadProgress,
        ModelStatus, Note, NoteSource, NoteSummary, PostProcessTemplate, PostprocessProviderMode,
        ProviderMode, SaveSettingsRequest, StartFileTranscriptionRequest,
        TranscribeLiveRecordingRequest, TranscriptResult, TranscriptionStreamEvent,
    },
    notes::NoteStore,
    providers::{
        api_openai_compatible::{resolve_effective_model_name, ApiCredentials},
        local_llm, local_whisper,
    },
    settings::SettingsStore,
    templates::TemplateStore,
    transcription::{
        cleanup_temporary_live_recording, file_source_name, finalize_live_transcription_result,
        live_source_name, transcribe_api_audio_path, transcribe_local_audio_path,
        TranscriptionMetadata,
    },
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
pub fn get_live_recording_status(
    live_recording_state: State<'_, live_recording::LiveRecordingManagerState>,
) -> LiveRecordingStatus {
    live_recording_state.current_status()
}

#[tauri::command]
pub fn start_live_transcription(
    app: tauri::AppHandle,
    settings_store: State<'_, SettingsStore>,
    live_recording_state: State<'_, live_recording::LiveRecordingManagerState>,
    monitor_state: State<'_, AudioMonitorState>,
) -> Result<LiveRecordingStatus, String> {
    // Release the audio monitor before opening the recording stream.
    // Prevents device-busy errors on platforms without concurrent stream
    // support (e.g. Linux/ALSA).
    monitor_state.stop();

    let settings = settings_store.load().map_err(|error| error.to_string())?;
    let selected_id = settings.selected_input_device_id.clone();

    let devices = input_devices::list_input_devices().ok().unwrap_or_default();
    let selected_device = selected_id
        .as_deref()
        .and_then(|id| devices.iter().find(|device| device.id == id));
    let is_output_loopback = selected_device
        .map(|device| device.is_output_loopback)
        .unwrap_or(false);
    let use_dual_capture = matches!(
        settings.live_capture_profile,
        LiveCaptureProfile::MeetingMix
    ) && !is_output_loopback
        && input_devices::platform_supports_output_loopback();

    live_recording_state
        .start(
            &app,
            selected_id.as_deref(),
            is_output_loopback,
            use_dual_capture,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_live_transcription(
    app: tauri::AppHandle,
    live_recording_state: State<'_, live_recording::LiveRecordingManagerState>,
) -> Result<LiveRecordingResult, String> {
    live_recording_state
        .stop(&app)
        .map_err(|error| error.to_string())
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
    let file_path = PathBuf::from(&request.file_path);
    let source_name = file_source_name(file_path.as_path());
    let settings = settings_store.load().map_err(|e| e.to_string())?;

    match settings.provider_mode {
        ProviderMode::Local => {
            let engine_cache = Arc::clone(&engine_state.inner);
            transcribe_local_audio_path(
                engine_cache,
                settings.local_model_id,
                file_path,
                TranscriptionMetadata {
                    input_type: InputType::File,
                    live_capture_profile: None,
                    source_name,
                    duration_ms: None,
                },
                on_update,
            )
            .await
        }
        ProviderMode::Api => {
            let (model_name, credentials) = load_api_credentials(&settings, &settings_store)?;

            transcribe_api_audio_path(
                file_path,
                model_name,
                credentials,
                TranscriptionMetadata {
                    input_type: InputType::File,
                    live_capture_profile: None,
                    source_name,
                    duration_ms: None,
                },
                on_update,
            )
            .await
        }
    }
}

#[tauri::command]
pub async fn transcribe_live_recording(
    request: TranscribeLiveRecordingRequest,
    on_update: Channel<TranscriptionStreamEvent>,
    engine_state: State<'_, LocalEngineState>,
    settings_store: State<'_, SettingsStore>,
) -> Result<TranscriptResult, String> {
    let file_path = PathBuf::from(&request.file_path);
    let cleanup_path = file_path.clone();
    let settings = settings_store.load().map_err(|e| e.to_string())?;

    let source_name = Some(live_source_name(
        &request.input_device_label,
        request.input_device_id.as_deref(),
    ));

    let result = match settings.provider_mode {
        ProviderMode::Local => {
            let engine_cache = Arc::clone(&engine_state.inner);
            transcribe_local_audio_path(
                engine_cache,
                settings.local_model_id,
                file_path,
                TranscriptionMetadata {
                    input_type: InputType::Live,
                    live_capture_profile: Some(request.live_capture_profile),
                    source_name,
                    duration_ms: Some(request.duration_ms),
                },
                on_update,
            )
            .await
        }
        ProviderMode::Api => {
            let (model_name, credentials) = load_api_credentials(&settings, &settings_store)?;

            transcribe_api_audio_path(
                file_path,
                model_name,
                credentials,
                TranscriptionMetadata {
                    input_type: InputType::Live,
                    live_capture_profile: Some(request.live_capture_profile),
                    source_name,
                    duration_ms: Some(request.duration_ms),
                },
                on_update,
            )
            .await
        }
    };

    let cleanup_result = cleanup_temporary_live_recording(cleanup_path.as_path());

    finalize_live_transcription_result(result, cleanup_result, cleanup_path.as_path())
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

#[tauri::command]
pub fn list_templates(store: State<'_, TemplateStore>) -> Vec<PostProcessTemplate> {
    store.load()
}

#[tauri::command]
pub fn save_templates(
    templates: Vec<PostProcessTemplate>,
    store: State<'_, TemplateStore>,
) -> Result<(), String> {
    store.save(&templates).map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn run_postprocess(
    transcript_text: String,
    template_id: String,
    enable_thinking: bool,
    note_slot_assignments: HashMap<String, String>,
    app: tauri::AppHandle,
    template_store: State<'_, TemplateStore>,
    settings_store: State<'_, SettingsStore>,
    llm_server_state: State<'_, LlmServerState>,
    cancel_state: State<'_, PostprocessCancelState>,
    note_store: State<'_, NoteStore>,
) -> Result<String, String> {
    let templates = template_store.load();
    let template = crate::templates::find_template_by_id(&templates, &template_id)
        .ok_or_else(|| format!("Template not found: {template_id}"))?;

    let has_transcript = template.prompt.contains("{{transcript}}");
    let has_note_slots = !crate::templates::extract_note_slots(&template.prompt).is_empty();
    if !has_transcript && !has_note_slots {
        return Err(
            "Template prompt must contain {{transcript}} or at least one {{noteN}} slot."
                .to_string(),
        );
    }

    // Resolve only the slots that actually appear in the prompt (the frontend
    // map may contain stale entries for slots the user removed from the text).
    let required_slots = crate::templates::extract_note_slots(&template.prompt);
    let mut note_contents: HashMap<String, String> = HashMap::new();
    for slot_name in &required_slots {
        if let Some(note_id) = note_slot_assignments.get(slot_name) {
            let note = note_store.get(note_id).ok_or_else(|| {
                format!("Note not found for slot {slot_name} (it may have been deleted): {note_id}")
            })?;
            note_contents.insert(slot_name.clone(), note.content);
        }
    }

    let rendered_prompt =
        crate::templates::render_template(&template.prompt, &transcript_text, &note_contents)?;

    let settings = settings_store.load().map_err(|e| e.to_string())?;

    match settings.postprocess_provider_mode {
        PostprocessProviderMode::Api => {
            let api_key =
                settings_store
                    .get_api_key(&settings.api_base_url)
                    .map_err(|error| match &error {
                        crate::settings::SettingsError::Validation(_) => {
                            "No API key is configured. Add an API key in Settings to use post-processing."
                                .to_string()
                        }
                        _ => error.to_string(),
                    })?;

            let credentials = ApiCredentials {
                api_key,
                base_url: settings.api_base_url,
            };

            crate::providers::api_openai_compatible::post_process_transcript(
                &rendered_prompt,
                &settings.postprocess_model,
                &credentials,
            )
            .await
            .map_err(|e| e.to_string())
        }
        PostprocessProviderMode::LocalLlm => {
            // 1. Ensure sidecar is running with the configured model
            let port = llm_engine::ensure_server_running(
                &llm_server_state,
                &app,
                &settings.local_llm_model_id,
            )
            .await?;

            // 2. Create cancellation token and publish it so cancel_postprocess can reach it
            let cancel_token = CancellationToken::new();
            {
                cancel_state
                    .token
                    .lock()
                    .unwrap()
                    .replace(cancel_token.clone());
            }

            // 3. Send streaming chat completion to localhost sidecar
            let result = llm_engine::send_chat_completion(
                port,
                &rendered_prompt,
                cancel_token,
                enable_thinking,
            )
            .await;

            // 4. Clear token (no-op if cancel_postprocess already took it)
            {
                cancel_state.token.lock().unwrap().take();
            }

            result
        }
    }
}

// ---------------------------------------------------------------------------
// LLM model management & post-processing cancellation
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_local_llm_models() -> Vec<LocalModelDescriptor> {
    local_llm::LLM_MODEL_IDS
        .iter()
        .map(|id| {
            let downloaded = local_llm::expected_model_path(id)
                .map(|p| p.exists())
                .unwrap_or(false);

            LocalModelDescriptor {
                id: id.to_string(),
                label: local_llm::display_label(id).to_string(),
                engine: local_llm::ENGINE_ID.to_string(),
                downloaded,
                size_label: local_llm::size_label(id).to_string(),
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_llm_model_status(model_id: String) -> Result<ModelStatus, String> {
    local_llm::model_status(&model_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_llm_model(
    model_id: String,
    llm_server_state: State<'_, LlmServerState>,
) -> Result<(), String> {
    // Stop the sidecar if it is serving this model — on Windows the GGUF file
    // is memory-mapped and cannot be deleted while the process holds it open.
    llm_engine::stop_server_for_model(&llm_server_state, &model_id).await;
    local_llm::delete_model(&model_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ensure_llm_model_downloaded(
    model_id: String,
    on_progress: Channel<ModelDownloadProgress>,
) -> Result<(), String> {
    local_llm::download_model(&model_id, &on_progress)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn preload_local_llm_model(
    model_id: String,
    app: tauri::AppHandle,
    llm_server_state: State<'_, LlmServerState>,
) -> Result<(), String> {
    llm_engine::ensure_server_running(&llm_server_state, &app, &model_id)
        .await
        .map(|_| ())
}

#[tauri::command]
pub fn cancel_postprocess(cancel_state: State<'_, PostprocessCancelState>) {
    if let Some(token) = cancel_state.token.lock().unwrap().take() {
        token.cancel();
    }
}

#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    let file_path = Path::new(&path);
    for component in file_path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err("File path must not contain \"..\" components.".to_string());
        }
    }
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::write(file_path, content).map_err(|e| format!("Failed to write file: {e}"))
}

#[tauri::command]
pub fn list_notes(store: State<'_, NoteStore>) -> Vec<NoteSummary> {
    store.list()
}

#[tauri::command]
pub fn get_note(id: String, store: State<'_, NoteStore>) -> Option<Note> {
    store.get(&id)
}

#[tauri::command]
pub fn create_note(
    title: String,
    content: String,
    source: NoteSource,
    store: State<'_, NoteStore>,
) -> Result<Note, String> {
    let now = crate::notes::iso_now();
    let note = Note {
        id: crate::notes::generate_note_id(),
        title,
        content,
        created_at: now.clone(),
        updated_at: now,
        source,
    };
    store.save(&note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_note(
    id: String,
    title: String,
    content: String,
    store: State<'_, NoteStore>,
) -> Result<Note, String> {
    let mut note = store
        .get(&id)
        .ok_or_else(|| format!("Note not found: {id}"))?;
    note.title = title;
    note.content = content;
    note.updated_at = crate::notes::iso_now();
    store.save(&note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(id: String, store: State<'_, NoteStore>) -> Result<(), String> {
    store.delete(&id).map_err(|e| e.to_string())
}

fn load_api_credentials(
    settings: &AppSettings,
    settings_store: &SettingsStore,
) -> Result<(String, ApiCredentials), String> {
    let model_name =
        resolve_effective_model_name(&settings.api_model_id, &settings.api_custom_model_name)
            .map_err(|e| e.to_string())?;
    let api_key = settings_store
        .get_api_key(&settings.api_base_url)
        .map_err(|e| e.to_string())?;
    Ok((
        model_name,
        ApiCredentials {
            api_key,
            base_url: settings.api_base_url.clone(),
        },
    ))
}

#[tauri::command]
pub fn start_audio_monitor(
    app: tauri::AppHandle,
    device_id: Option<String>,
    monitor_state: State<'_, AudioMonitorState>,
) -> Result<(), String> {
    monitor_state
        .start(&app, device_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_audio_monitor(monitor_state: State<'_, AudioMonitorState>) -> Result<(), String> {
    monitor_state.stop();
    Ok(())
}
