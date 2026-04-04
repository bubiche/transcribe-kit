use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::State;

use crate::{
    audio, hotkeys, input_devices, live_recording,
    models::{
        ApiModelDescriptor, AppSettings, AudioInputDeviceDescriptor, InputType, LiveCaptureProfile,
        LiveRecordingResult, LiveRecordingStatus, LocalModelDescriptor, ModelDownloadProgress,
        ModelStatus, ProviderMode, SaveSettingsRequest, StartFileTranscriptionRequest,
        TranscribeLiveRecordingRequest, TranscriptResult, TranscriptionStreamEvent,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalTranscriptionMetadata {
    input_type: InputType,
    live_capture_profile: Option<LiveCaptureProfile>,
    source_name: Option<String>,
    duration_ms: Option<u64>,
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
) -> Result<LiveRecordingStatus, String> {
    let settings = settings_store.load().map_err(|error| error.to_string())?;
    live_recording_state
        .start(&app, settings.selected_input_device_id.as_deref())
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
    let model_id = local_model_id(&settings_store, "File import transcription")?;
    let engine_cache = Arc::clone(&engine_state.inner);

    transcribe_local_audio_path(
        engine_cache,
        model_id,
        file_path,
        LocalTranscriptionMetadata {
            input_type: InputType::File,
            live_capture_profile: None,
            source_name,
            duration_ms: None,
        },
        on_update,
    )
    .await
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
    let result = match local_model_id(&settings_store, "Live recording transcription") {
        Ok(model_id) => {
            let engine_cache = Arc::clone(&engine_state.inner);
            transcribe_local_audio_path(
                engine_cache,
                model_id,
                file_path,
                LocalTranscriptionMetadata {
                    input_type: InputType::Live,
                    live_capture_profile: Some(request.live_capture_profile),
                    source_name: Some(live_source_name(
                        &request.input_device_label,
                        request.input_device_id.as_deref(),
                    )),
                    duration_ms: Some(request.duration_ms),
                },
                on_update,
            )
            .await
        }
        Err(error) => Err(error),
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

fn local_model_id(
    settings_store: &State<'_, SettingsStore>,
    transcription_label: &str,
) -> Result<String, String> {
    let settings = settings_store.load().map_err(|e| e.to_string())?;

    if settings.provider_mode != ProviderMode::Local {
        return Err(format!(
            "{transcription_label} is only wired up for Local Whisper right now. Switch the provider in Settings to continue."
        ));
    }

    Ok(settings.local_model_id)
}

async fn transcribe_local_audio_path(
    engine_cache: Arc<Mutex<Option<WhisperEngine>>>,
    model_id: String,
    file_path: PathBuf,
    metadata: LocalTranscriptionMetadata,
    on_update: Channel<TranscriptionStreamEvent>,
) -> Result<TranscriptResult, String> {
    let on_update = Arc::new(on_update);

    tokio::task::spawn_blocking(move || {
        transcribe_local_audio_path_blocking(
            &engine_cache,
            &model_id,
            file_path.as_path(),
            metadata,
            &on_update,
        )
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))?
}

fn transcribe_local_audio_path_blocking(
    engine_cache: &Arc<Mutex<Option<WhisperEngine>>>,
    model_id: &str,
    file_path: &Path,
    metadata: LocalTranscriptionMetadata,
    on_update: &Arc<Channel<TranscriptionStreamEvent>>,
) -> Result<TranscriptResult, String> {
    let engine = get_or_load_engine(engine_cache, model_id)?;
    let decoded_audio = audio::decode_audio_file(file_path).map_err(|e| e.to_string())?;
    let progress_updates = Arc::clone(on_update);
    let segment_updates = Arc::clone(on_update);
    let result = engine
        .transcribe_pcm_streaming(
            &decoded_audio.samples,
            Some(move |progress_percent| {
                let _ =
                    progress_updates.send(TranscriptionStreamEvent::Progress { progress_percent });
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
    Ok(apply_transcription_metadata(
        result,
        metadata,
        decoded_audio.duration_ms,
    ))
}

fn file_source_name(file_path: &Path) -> Option<String> {
    file_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
}

fn live_source_name(input_device_label: &str, input_device_id: Option<&str>) -> String {
    let trimmed_label = input_device_label.trim();
    if !trimmed_label.is_empty() {
        return trimmed_label.to_string();
    }

    let trimmed_id = input_device_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(device_id) = trimmed_id {
        return device_id.to_string();
    }

    "Live recording".to_string()
}

fn cleanup_temporary_live_recording(file_path: &Path) -> Result<(), String> {
    match std::fs::remove_file(file_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn apply_transcription_metadata(
    mut result: TranscriptResult,
    metadata: LocalTranscriptionMetadata,
    decoded_duration_ms: Option<u64>,
) -> TranscriptResult {
    result.source.input_type = metadata.input_type;
    result.source.live_capture_profile = metadata.live_capture_profile;
    result.source.source_name = metadata.source_name;
    result.source.duration_ms = metadata.duration_ms.or(decoded_duration_ms);
    result
}

fn finalize_live_transcription_result(
    result: Result<TranscriptResult, String>,
    cleanup_result: Result<(), String>,
    cleanup_path: &Path,
) -> Result<TranscriptResult, String> {
    match (result, cleanup_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Ok(result), Err(cleanup_error)) => {
            eprintln!(
                "Live transcription completed, but Transcribe Kit could not delete the temporary WAV at {}: {}",
                cleanup_path.display(),
                cleanup_error
            );
            Ok(result)
        }
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => Err(format!(
            "{error} Transcribe Kit also could not delete the temporary live recording: {cleanup_error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_transcription_metadata, cleanup_temporary_live_recording, file_source_name,
        finalize_live_transcription_result, live_source_name, LocalTranscriptionMetadata,
    };
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::models::{InputType, LiveCaptureProfile, TranscriptResult, TranscriptionSource};

    #[test]
    fn live_source_name_prefers_trimmed_device_label() {
        assert_eq!(
            live_source_name("  Built-in Microphone  ", Some("device-123")),
            "Built-in Microphone"
        );
    }

    #[test]
    fn live_source_name_falls_back_to_device_id_then_default() {
        assert_eq!(live_source_name("   ", Some(" device-123 ")), "device-123");
        assert_eq!(live_source_name("", None), "Live recording");
    }

    #[test]
    fn file_source_name_uses_file_name_only() {
        let path = PathBuf::from("/tmp/transcribe-kit/example.wav");
        assert_eq!(
            file_source_name(path.as_path()).as_deref(),
            Some("example.wav")
        );
    }

    #[test]
    fn cleanup_temporary_live_recording_removes_existing_file() {
        let path = unique_temp_test_path("cleanup-existing");
        fs::write(&path, b"wav").expect("create temp test file");

        cleanup_temporary_live_recording(path.as_path()).expect("cleanup succeeds");

        assert!(!path.exists(), "expected temp test file to be removed");
    }

    #[test]
    fn cleanup_temporary_live_recording_ignores_missing_file() {
        let path = unique_temp_test_path("cleanup-missing");
        if path.exists() {
            fs::remove_file(&path).expect("remove leftover temp test file");
        }

        cleanup_temporary_live_recording(path.as_path()).expect("cleanup succeeds");
    }

    #[test]
    fn apply_transcription_metadata_preserves_live_profile_and_explicit_metadata() {
        let result = apply_transcription_metadata(
            sample_transcript_result(),
            LocalTranscriptionMetadata {
                input_type: InputType::Live,
                live_capture_profile: Some(LiveCaptureProfile::MeetingMix),
                source_name: Some("Desk Mic".to_string()),
                duration_ms: Some(8_200),
            },
            Some(7_900),
        );

        assert_eq!(result.source.input_type, InputType::Live);
        assert_eq!(
            result.source.live_capture_profile,
            Some(LiveCaptureProfile::MeetingMix)
        );
        assert_eq!(result.source.source_name.as_deref(), Some("Desk Mic"));
        assert_eq!(result.source.duration_ms, Some(8_200));
    }

    #[test]
    fn apply_transcription_metadata_falls_back_to_decoded_duration() {
        let result = apply_transcription_metadata(
            sample_transcript_result(),
            LocalTranscriptionMetadata {
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: Some("note.wav".to_string()),
                duration_ms: None,
            },
            Some(4_500),
        );

        assert_eq!(result.source.input_type, InputType::File);
        assert_eq!(result.source.live_capture_profile, None);
        assert_eq!(result.source.source_name.as_deref(), Some("note.wav"));
        assert_eq!(result.source.duration_ms, Some(4_500));
    }

    #[test]
    fn finalize_live_transcription_result_keeps_success_when_cleanup_fails() {
        let result = finalize_live_transcription_result(
            Ok(sample_transcript_result()),
            Err("permission denied".to_string()),
            PathBuf::from("/tmp/live.wav").as_path(),
        )
        .expect("successful transcript should be preserved");

        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn finalize_live_transcription_result_combines_cleanup_error_with_failure() {
        let error = finalize_live_transcription_result(
            Err("provider mismatch".to_string()),
            Err("permission denied".to_string()),
            PathBuf::from("/tmp/live.wav").as_path(),
        )
        .expect_err("expected combined failure");

        assert_eq!(
            error,
            "provider mismatch Transcribe Kit also could not delete the temporary live recording: permission denied"
        );
    }

    fn unique_temp_test_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();

        std::env::temp_dir().join(format!("transcribe-kit-{label}-{unique}.tmp"))
    }

    fn sample_transcript_result() -> TranscriptResult {
        TranscriptResult {
            text: "hello world".to_string(),
            segments: Vec::new(),
            source: TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: None,
                duration_ms: None,
            },
            post_processed_text: None,
        }
    }
}
