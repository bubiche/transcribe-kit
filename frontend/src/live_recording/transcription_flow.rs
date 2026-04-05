use leptos::prelude::Set;

use crate::tauri_api::{InputType, LiveRecordingResult, LiveRecordingStatus, TranscriptResult};

use super::timing::{format_duration, idle_status};
use super::{LiveRecordingController, LiveRecordingSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StopUiTransition {
    pub(super) status: LiveRecordingStatus,
    pub(super) armed_input_label: Option<String>,
    pub(super) last_result: Option<LiveRecordingSummary>,
    pub(super) feedback_message: Option<String>,
    pub(super) error_message: Option<String>,
}

pub(super) fn stop_ui_transition(
    result: Result<LiveRecordingResult, String>,
    expected_dual_capture: bool,
) -> StopUiTransition {
    match result {
        Ok(result) => StopUiTransition {
            status: idle_status(),
            armed_input_label: Some(result.input_device_label.clone()),
            last_result: Some(live_recording_summary(&result)),
            feedback_message: Some(stop_capture_feedback_message(
                &result,
                expected_dual_capture,
            )),
            error_message: None,
        },
        Err(error) => StopUiTransition {
            status: idle_status(),
            armed_input_label: None,
            last_result: None,
            feedback_message: None,
            error_message: Some(format!("Live capture did not stop cleanly: {error}")),
        },
    }
}

pub(super) fn apply_stop_ui_transition(
    controller: LiveRecordingController,
    transition: &StopUiTransition,
) {
    if let Some(label) = transition.armed_input_label.clone() {
        controller.armed_input_label.set(label);
    }

    controller.status.set(transition.status.clone());
    controller.recording_started_at_ms.set(None);
    controller.last_result.set(transition.last_result.clone());
    controller
        .feedback_message
        .set(transition.feedback_message.clone());
    controller
        .error_message
        .set(transition.error_message.clone());
}

pub(super) fn apply_live_transcription_started(
    controller: LiveRecordingController,
    source_name: &str,
) {
    controller.error_message.set(None);
    controller.feedback_message.set(None);
    controller
        .transcription
        .start_live_job(source_name.to_string());
}

pub(super) fn apply_live_transcription_succeeded(
    controller: LiveRecordingController,
    source_name: &str,
    result: TranscriptResult,
) {
    controller.transcription.complete_job(result);
    controller.error_message.set(None);
    controller.feedback_message.set(Some(format!(
        "Live transcript ready for review from {source_name}."
    )));
}

pub(super) fn apply_live_transcription_failed(
    controller: LiveRecordingController,
    source_name: &str,
    error: &str,
) {
    controller.transcription.fail_job(
        InputType::Live,
        Some(source_name.to_string()),
        error.to_string(),
    );
    controller.feedback_message.set(None);
    controller
        .error_message
        .set(Some(format!("Live transcription failed: {error}")));
}

pub(super) fn live_transcription_source_name(result: &LiveRecordingResult) -> String {
    let trimmed_label = result.input_device_label.trim();
    if !trimmed_label.is_empty() {
        return trimmed_label.to_string();
    }

    let trimmed_id = result
        .input_device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(device_id) = trimmed_id {
        return device_id.to_string();
    }

    "Live recording".to_string()
}

fn live_recording_summary(result: &LiveRecordingResult) -> LiveRecordingSummary {
    LiveRecordingSummary {
        input_device_label: live_transcription_source_name(result),
        sample_rate_hz: result.sample_rate_hz,
        channels: result.channels,
        duration_ms: result.duration_ms,
    }
}

fn stop_capture_feedback_message(
    result: &LiveRecordingResult,
    expected_dual_capture: bool,
) -> String {
    if expected_dual_capture && !result.is_dual_capture {
        #[cfg(target_os = "macos")]
        {
            return format!(
                "Capture stopped, but system audio was not captured. Transcribe Kit fell back to microphone-only recording from {} ({}, {} Hz, {} ch). Enable System Audio Recording for Transcribe Kit in System Settings > Privacy & Security, then try again.",
                result.input_device_label,
                format_duration(result.duration_ms),
                result.sample_rate_hz,
                result.channels
            );
        }

        #[cfg(not(target_os = "macos"))]
        {
            return format!(
                "Capture stopped, but system audio was not captured. Transcribe Kit fell back to microphone-only recording from {} ({}, {} Hz, {} ch). Check system audio capture permissions and try again.",
                result.input_device_label,
                format_duration(result.duration_ms),
                result.sample_rate_hz,
                result.channels
            );
        }
    }

    format!(
        "Capture stopped. Temporary WAV saved from {} ({}, {} Hz, {} ch).",
        result.input_device_label,
        format_duration(result.duration_ms),
        result.sample_rate_hz,
        result.channels
    )
}
