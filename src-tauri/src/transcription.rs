use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;

use crate::audio;
use crate::engine::get_or_load_engine;
use crate::models::{InputType, LiveCaptureProfile, TranscriptResult, TranscriptionStreamEvent};
use crate::providers::api_openai_compatible::ApiCredentials;
use crate::providers::local_whisper::WhisperEngine;
use crate::providers::transcribe_api_audio_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptionMetadata {
    pub input_type: InputType,
    pub live_capture_profile: Option<LiveCaptureProfile>,
    pub source_name: Option<String>,
    pub duration_ms: Option<u64>,
}

pub(crate) async fn transcribe_local_audio_path(
    engine_cache: Arc<Mutex<Option<WhisperEngine>>>,
    model_id: String,
    file_path: PathBuf,
    metadata: TranscriptionMetadata,
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
    metadata: TranscriptionMetadata,
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

pub(crate) async fn transcribe_api_audio_path(
    file_path: PathBuf,
    model_name: String,
    credentials: ApiCredentials,
    metadata: TranscriptionMetadata,
    on_update: Channel<TranscriptionStreamEvent>,
) -> Result<TranscriptResult, String> {
    let _ = on_update.send(TranscriptionStreamEvent::Progress {
        progress_percent: 0,
    });

    let result = transcribe_api_audio_file(&file_path, &model_name, &credentials)
        .await
        .map_err(|e| e.to_string())?;

    let _ = on_update.send(TranscriptionStreamEvent::Progress {
        progress_percent: 100,
    });

    let api_duration_ms = result.source.duration_ms;
    Ok(apply_transcription_metadata(
        result,
        metadata,
        api_duration_ms,
    ))
}

pub(crate) fn file_source_name(file_path: &Path) -> Option<String> {
    file_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
}

pub(crate) fn live_source_name(input_device_label: &str, input_device_id: Option<&str>) -> String {
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

pub(crate) fn cleanup_temporary_live_recording(file_path: &Path) -> Result<(), String> {
    match std::fs::remove_file(file_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn apply_transcription_metadata(
    mut result: TranscriptResult,
    metadata: TranscriptionMetadata,
    decoded_duration_ms: Option<u64>,
) -> TranscriptResult {
    result.source.input_type = metadata.input_type;
    result.source.live_capture_profile = metadata.live_capture_profile;
    result.source.source_name = metadata.source_name;
    result.source.duration_ms = metadata.duration_ms.or(decoded_duration_ms);
    result
}

pub(crate) fn finalize_live_transcription_result(
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
        finalize_live_transcription_result, live_source_name, TranscriptionMetadata,
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
            TranscriptionMetadata {
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
            TranscriptionMetadata {
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
