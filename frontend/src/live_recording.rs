#[cfg(target_arch = "wasm32")]
use js_sys::Date;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

use crate::features::transcription::TranscriptionController;
use crate::tauri_api::{
    get_live_recording_status, get_settings, list_input_devices, listen_to_app_event,
    start_live_transcription, stop_live_transcription, transcribe_live_recording, AppSettings,
    AudioInputDeviceDescriptor, HotkeyActivityEvent, HotkeyActivityState, HotkeyMode, InputType,
    LiveRecordingResult, LiveRecordingState, LiveRecordingStatus, TranscribeLiveRecordingRequest,
    TranscriptResult,
};

pub const HOTKEY_ACTIVITY_EVENT_NAME: &str = "transcribe-kit://live-recording-hotkey";
pub const LIVE_RECORDING_STATUS_EVENT_NAME: &str = "transcribe-kit://live-recording-status";

#[derive(Clone, Copy)]
pub struct LiveRecordingController {
    pub status: RwSignal<LiveRecordingStatus>,
    pub armed_input_label: RwSignal<String>,
    pub recording_started_at_ms: RwSignal<Option<f64>>,
    pub feedback_message: RwSignal<Option<String>>,
    pub error_message: RwSignal<Option<String>>,
    pub load_error: RwSignal<Option<String>>,
    pub device_context_error: RwSignal<Option<String>>,
    pub last_result: RwSignal<Option<LiveRecordingSummary>>,
    pub is_ready: RwSignal<bool>,
    pub transcription: TranscriptionController,
    desired_recording: RwSignal<bool>,
    request_in_flight: RwSignal<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveRecordingCommand {
    Start,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StopUiTransition {
    status: LiveRecordingStatus,
    armed_input_label: Option<String>,
    last_result: Option<LiveRecordingSummary>,
    feedback_message: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRecordingSummary {
    pub input_device_label: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

impl LiveRecordingController {
    pub fn new(transcription: TranscriptionController) -> Self {
        Self {
            status: RwSignal::new(idle_status()),
            armed_input_label: RwSignal::new("System default input".to_string()),
            recording_started_at_ms: RwSignal::new(None),
            feedback_message: RwSignal::new(None),
            error_message: RwSignal::new(None),
            load_error: RwSignal::new(None),
            device_context_error: RwSignal::new(None),
            last_result: RwSignal::new(None),
            is_ready: RwSignal::new(false),
            transcription,
            desired_recording: RwSignal::new(false),
            request_in_flight: RwSignal::new(false),
        }
    }

    pub fn initialize(self) {
        self.refresh_armed_device_context();

        spawn_local(async move {
            match get_live_recording_status().await {
                Ok(status) => {
                    self.desired_recording
                        .set(matches!(status.state, LiveRecordingState::Recording));
                    self.apply_status(status);
                }
                Err(error) => {
                    self.load_error.set(Some(format!(
                        "Live recording status could not be loaded: {error}"
                    )));
                }
            }

            self.is_ready.set(true);
        });

        spawn_local(async move {
            let _ = listen_to_app_event(HOTKEY_ACTIVITY_EVENT_NAME, {
                let controller = self;
                move |value: JsValue| {
                    let Ok(event) = serde_wasm_bindgen::from_value::<HotkeyActivityEvent>(value)
                    else {
                        return;
                    };

                    let current_recording = matches!(
                        controller.status.get_untracked().state,
                        LiveRecordingState::Recording
                    );
                    let desired = controller.desired_recording.get_untracked();

                    let Some(next_goal) =
                        desired_recording_goal(event.mode, event.state, current_recording, desired)
                    else {
                        return;
                    };

                    controller.desired_recording.set(next_goal);
                    reconcile_recording_goal(controller);
                }
            })
            .await;
        });

        spawn_local(async move {
            let _ = listen_to_app_event(LIVE_RECORDING_STATUS_EVENT_NAME, {
                let controller = self;
                move |value: JsValue| {
                    let Ok(status) = serde_wasm_bindgen::from_value::<LiveRecordingStatus>(value)
                    else {
                        return;
                    };

                    if !controller.request_in_flight.get_untracked() {
                        controller
                            .desired_recording
                            .set(matches!(status.state, LiveRecordingState::Recording));
                    }
                    controller.apply_status(status);
                }
            })
            .await;
        });
    }

    pub fn toggle_recording(self) {
        let is_recording = matches!(
            self.status.get_untracked().state,
            LiveRecordingState::Recording
        );
        self.desired_recording.set(!is_recording);
        reconcile_recording_goal(self);
    }

    pub fn refresh_armed_device_context(self) {
        spawn_local(async move {
            let settings_result = get_settings().await;
            let devices_result = list_input_devices().await;

            match (settings_result, devices_result) {
                (Ok(settings), Ok(devices)) => {
                    self.armed_input_label
                        .set(resolve_armed_input_label(&settings, &devices));
                    self.device_context_error.set(None);
                }
                (Err(settings_error), Err(devices_error)) => {
                    self.device_context_error.set(Some(format!(
                        "Saved recording device context could not be refreshed: settings: {settings_error} | input devices: {devices_error}"
                    )));
                }
                (Err(error), _) => {
                    self.device_context_error.set(Some(format!(
                        "Saved recording settings could not be refreshed: {error}"
                    )));
                }
                (_, Err(error)) => {
                    self.device_context_error.set(Some(format!(
                        "Available audio inputs could not be refreshed: {error}"
                    )));
                }
            }
        });
    }

    fn apply_status(self, status: LiveRecordingStatus) {
        self.load_error.set(None);

        if let Some(label) = status.input_device_label.clone() {
            self.armed_input_label.set(label);
            self.device_context_error.set(None);
        }

        if matches!(status.state, LiveRecordingState::Recording) {
            let baseline_duration_ms = status.duration_ms.unwrap_or_default() as f64;
            self.recording_started_at_ms
                .set(Some((now_ms() - baseline_duration_ms).max(0.0)));
            self.error_message.set(None);
            self.feedback_message.set(None);
            self.last_result.set(None);
        } else {
            self.recording_started_at_ms.set(None);
        }

        if let Some(message) = status.message.clone() {
            self.error_message.set(Some(message));
        }

        self.status.set(status);
    }
}

fn reconcile_recording_goal(controller: LiveRecordingController) {
    let is_recording = matches!(
        controller.status.get_untracked().state,
        LiveRecordingState::Recording
    );
    if controller.desired_recording.get_untracked()
        && controller.transcription.is_transcribing.get_untracked()
    {
        controller.desired_recording.set(false);
        controller.error_message.set(None);
        controller.feedback_message.set(Some(
            "Wait for the current transcription job to finish before starting another live capture."
                .to_string(),
        ));
        return;
    }

    if controller.request_in_flight.get_untracked() {
        return;
    }

    let Some(command) =
        next_command_for_goal(is_recording, controller.desired_recording.get_untracked())
    else {
        return;
    };

    controller.request_in_flight.set(true);
    controller.error_message.set(None);

    match command {
        LiveRecordingCommand::Start => {
            controller.last_result.set(None);
            controller
                .feedback_message
                .set(Some("Starting live capture...".to_string()));

            spawn_local(async move {
                match start_live_transcription().await {
                    Ok(status) => {
                        controller.apply_status(status);
                    }
                    Err(error) => {
                        controller.desired_recording.set(false);
                        controller.status.set(idle_status());
                        controller.recording_started_at_ms.set(None);
                        controller
                            .error_message
                            .set(Some(format!("Live capture did not start: {error}")));
                        controller.feedback_message.set(None);
                        controller.refresh_armed_device_context();
                    }
                }

                controller.request_in_flight.set(false);
                reconcile_recording_goal(controller);
            });
        }
        LiveRecordingCommand::Stop => {
            controller
                .feedback_message
                .set(Some("Stopping live capture...".to_string()));

            spawn_local(async move {
                match stop_live_transcription().await {
                    Ok(result) => {
                        let transition = stop_ui_transition(Ok(result.clone()));
                        apply_stop_ui_transition(controller, &transition);

                        if transition.armed_input_label.is_some() {
                            controller.refresh_armed_device_context();
                        }

                        run_live_transcription(controller, result).await;
                    }
                    Err(error) => {
                        let transition = stop_ui_transition(Err(error));
                        apply_stop_ui_transition(controller, &transition);
                    }
                }

                controller.request_in_flight.set(false);
                reconcile_recording_goal(controller);
            });
        }
    }
}

fn resolve_armed_input_label(
    settings: &AppSettings,
    devices: &[AudioInputDeviceDescriptor],
) -> String {
    match settings.selected_input_device_id.as_deref() {
        Some(selected_id) => devices
            .iter()
            .find(|device| device.id == selected_id)
            .map(|device| device.label.clone())
            .unwrap_or_else(|| "Previously selected input unavailable".to_string()),
        None => devices
            .iter()
            .find(|device| device.is_default)
            .map(|device| format!("System default ({})", device.label))
            .unwrap_or_else(|| "System default input".to_string()),
    }
}

fn desired_recording_goal(
    mode: HotkeyMode,
    state: HotkeyActivityState,
    current_recording: bool,
    desired_recording: bool,
) -> Option<bool> {
    match mode {
        HotkeyMode::PushToTalk => Some(matches!(state, HotkeyActivityState::Pressed)),
        HotkeyMode::Toggle => match state {
            HotkeyActivityState::Pressed => Some(if current_recording == desired_recording {
                !current_recording
            } else {
                !desired_recording
            }),
            HotkeyActivityState::Released => None,
        },
    }
}

fn next_command_for_goal(
    is_recording: bool,
    desired_recording: bool,
) -> Option<LiveRecordingCommand> {
    match (is_recording, desired_recording) {
        (false, true) => Some(LiveRecordingCommand::Start),
        (true, false) => Some(LiveRecordingCommand::Stop),
        _ => None,
    }
}

fn idle_status() -> LiveRecordingStatus {
    LiveRecordingStatus {
        state: LiveRecordingState::Idle,
        input_device_id: None,
        input_device_label: None,
        output_file_path: None,
        sample_rate_hz: None,
        channels: None,
        duration_ms: None,
        message: None,
    }
}

pub fn format_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

pub fn live_elapsed_duration_ms(
    status: &LiveRecordingStatus,
    recording_started_at_ms: Option<f64>,
) -> u64 {
    if !matches!(status.state, LiveRecordingState::Recording) {
        return status.duration_ms.unwrap_or_default();
    }

    let wall_clock_duration_ms = recording_started_at_ms
        .map(|started_at_ms| (now_ms() - started_at_ms).max(0.0) as u64)
        .unwrap_or_default();

    wall_clock_duration_ms.max(status.duration_ms.unwrap_or_default())
}

fn stop_ui_transition(result: Result<LiveRecordingResult, String>) -> StopUiTransition {
    match result {
        Ok(result) => StopUiTransition {
            status: idle_status(),
            armed_input_label: Some(result.input_device_label.clone()),
            last_result: Some(live_recording_summary(&result)),
            feedback_message: Some(format!(
                "Capture stopped. Temporary WAV saved from {} ({}, {} Hz, {} ch).",
                result.input_device_label,
                format_duration(result.duration_ms),
                result.sample_rate_hz,
                result.channels
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

async fn run_live_transcription(
    controller: LiveRecordingController,
    capture_result: LiveRecordingResult,
) {
    let source_name = live_transcription_source_name(&capture_result);
    apply_live_transcription_started(controller, &source_name);

    let progress_controller = controller.transcription;
    match transcribe_live_recording(
        TranscribeLiveRecordingRequest {
            file_path: capture_result.file_path.clone(),
            input_device_id: capture_result.input_device_id.clone(),
            input_device_label: capture_result.input_device_label.clone(),
            duration_ms: capture_result.duration_ms,
        },
        move |event| {
            progress_controller.apply_stream_event(event);
        },
    )
    .await
    {
        Ok(result) => apply_live_transcription_succeeded(controller, &source_name, result),
        Err(error) => apply_live_transcription_failed(controller, &source_name, &error),
    }
}

fn apply_live_transcription_started(controller: LiveRecordingController, source_name: &str) {
    controller.error_message.set(None);
    controller.feedback_message.set(None);
    controller
        .transcription
        .start_live_job(source_name.to_string());
}

fn apply_live_transcription_succeeded(
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

fn apply_live_transcription_failed(
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

fn live_transcription_source_name(result: &LiveRecordingResult) -> String {
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

fn apply_stop_ui_transition(controller: LiveRecordingController, transition: &StopUiTransition) {
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

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        Date::now()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};

        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tauri_api::{TranscriptSegment, TranscriptionJobState, TranscriptionSource};

    #[test]
    fn push_to_talk_press_starts_recording() {
        let goal = desired_recording_goal(
            HotkeyMode::PushToTalk,
            HotkeyActivityState::Pressed,
            false,
            false,
        );

        assert_eq!(goal, Some(true));
    }

    #[test]
    fn push_to_talk_release_stops_recording() {
        let goal = desired_recording_goal(
            HotkeyMode::PushToTalk,
            HotkeyActivityState::Released,
            true,
            true,
        );

        assert_eq!(goal, Some(false));
    }

    #[test]
    fn toggle_press_starts_when_idle() {
        let goal = desired_recording_goal(
            HotkeyMode::Toggle,
            HotkeyActivityState::Pressed,
            false,
            false,
        );

        assert_eq!(goal, Some(true));
    }

    #[test]
    fn toggle_press_stops_when_already_recording() {
        let goal =
            desired_recording_goal(HotkeyMode::Toggle, HotkeyActivityState::Pressed, true, true);

        assert_eq!(goal, Some(false));
    }

    #[test]
    fn toggle_release_does_not_change_goal() {
        let goal = desired_recording_goal(
            HotkeyMode::Toggle,
            HotkeyActivityState::Released,
            true,
            true,
        );

        assert_eq!(goal, None);
    }

    #[test]
    fn toggle_press_flips_pending_goal_while_start_is_in_flight() {
        let goal = desired_recording_goal(
            HotkeyMode::Toggle,
            HotkeyActivityState::Pressed,
            false,
            true,
        );

        assert_eq!(goal, Some(false));
    }

    #[test]
    fn live_elapsed_duration_uses_wall_clock_when_recording() {
        let status = LiveRecordingStatus {
            state: LiveRecordingState::Recording,
            input_device_id: None,
            input_device_label: None,
            output_file_path: None,
            sample_rate_hz: None,
            channels: None,
            duration_ms: Some(1_000),
            message: None,
        };

        let elapsed = live_elapsed_duration_ms(&status, Some(now_ms() - 3_200.0));

        assert!(elapsed >= 3_000);
    }

    #[test]
    fn live_elapsed_duration_falls_back_to_status_when_idle() {
        let status = LiveRecordingStatus {
            state: LiveRecordingState::Idle,
            input_device_id: None,
            input_device_label: None,
            output_file_path: None,
            sample_rate_hz: None,
            channels: None,
            duration_ms: Some(4_200),
            message: None,
        };

        assert_eq!(live_elapsed_duration_ms(&status, Some(now_ms())), 4_200);
    }

    #[test]
    fn reconcile_recording_goal_rejects_new_start_while_transcribing() {
        let transcription = TranscriptionController::new();
        transcription.is_transcribing.set(true);
        transcription
            .job_status
            .update(|status| status.input_type = InputType::Live);

        let controller = LiveRecordingController::new(transcription);
        controller.desired_recording.set(true);
        controller.request_in_flight.set(true);

        reconcile_recording_goal(controller);

        assert!(!controller.desired_recording.get_untracked());
        assert_eq!(
            controller.feedback_message.get().as_deref(),
            Some("Wait for the current transcription job to finish before starting another live capture.")
        );
        assert_eq!(controller.error_message.get(), None);
    }

    #[test]
    fn stop_failure_transition_returns_idle_error_state() {
        let transition = stop_ui_transition(Err("writer finalize failed".to_string()));

        assert_eq!(transition.status.state, LiveRecordingState::Idle);
        assert_eq!(
            transition.error_message.as_deref(),
            Some("Live capture did not stop cleanly: writer finalize failed")
        );
        assert_eq!(transition.feedback_message, None);
        assert_eq!(transition.last_result, None);
        assert_eq!(transition.armed_input_label, None);
    }

    #[test]
    fn stop_success_transition_returns_idle_success_state() {
        let transition = stop_ui_transition(Ok(LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some("mic-1".to_string()),
            input_device_label: "Desk Mic".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
        }));

        assert_eq!(transition.status.state, LiveRecordingState::Idle);
        assert_eq!(transition.error_message, None);
        assert_eq!(transition.armed_input_label.as_deref(), Some("Desk Mic"));
        assert_eq!(
            transition.feedback_message.as_deref(),
            Some("Capture stopped. Temporary WAV saved from Desk Mic (00:05, 48000 Hz, 2 ch).")
        );
        assert_eq!(
            transition.last_result.as_ref(),
            Some(&LiveRecordingSummary {
                input_device_label: "Desk Mic".to_string(),
                sample_rate_hz: 48_000,
                channels: 2,
                duration_ms: 5_200,
            })
        );
    }

    #[test]
    fn recording_to_transcribing_to_done_leaves_live_banner_idle_and_review_state_consistent() {
        let transcription = TranscriptionController::new();
        let controller = LiveRecordingController::new(transcription);
        controller.status.set(recording_status("Desk Mic"));
        controller
            .recording_started_at_ms
            .set(Some(now_ms() - 2_000.0));

        let capture_result = sample_capture_result();
        let transition = stop_ui_transition(Ok(capture_result));
        apply_stop_ui_transition(controller, &transition);
        apply_live_transcription_started(controller, "Desk Mic");

        assert_eq!(controller.status.get().state, LiveRecordingState::Idle);
        assert_eq!(controller.recording_started_at_ms.get(), None);
        assert_eq!(controller.feedback_message.get(), None);
        assert_eq!(controller.error_message.get(), None);
        assert!(controller.transcription.is_transcribing.get());
        assert_eq!(
            controller.transcription.job_status.get(),
            crate::tauri_api::TranscriptionJobStatus {
                state: TranscriptionJobState::Running,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("Transcribing Desk Mic".to_string()),
            }
        );

        let result = sample_transcript_result("Desk Mic", "final transcript");
        apply_live_transcription_succeeded(controller, "Desk Mic", result.clone());

        assert_eq!(controller.status.get().state, LiveRecordingState::Idle);
        assert!(!controller.transcription.is_transcribing.get());
        assert_eq!(controller.error_message.get(), None);
        assert_eq!(
            controller.feedback_message.get().as_deref(),
            Some("Live transcript ready for review from Desk Mic.")
        );
        assert_eq!(
            controller.transcription.transcript.get(),
            Some(result.clone())
        );
        assert_eq!(
            controller.transcription.job_status.get(),
            crate::tauri_api::TranscriptionJobStatus {
                state: TranscriptionJobState::Succeeded,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("Transcript ready for review.".to_string()),
            }
        );
        assert_eq!(
            controller.last_result.get(),
            Some(LiveRecordingSummary {
                input_device_label: "Desk Mic".to_string(),
                sample_rate_hz: 48_000,
                channels: 2,
                duration_ms: 5_200,
            })
        );
    }

    #[test]
    fn recording_to_transcribing_to_failed_leaves_live_banner_idle_and_not_recording() {
        let transcription = TranscriptionController::new();
        let controller = LiveRecordingController::new(transcription);
        controller.status.set(recording_status("Desk Mic"));
        controller
            .recording_started_at_ms
            .set(Some(now_ms() - 2_000.0));

        let transition = stop_ui_transition(Ok(sample_capture_result()));
        apply_stop_ui_transition(controller, &transition);
        apply_live_transcription_started(controller, "Desk Mic");
        apply_live_transcription_failed(controller, "Desk Mic", "provider mismatch");

        assert_eq!(controller.status.get().state, LiveRecordingState::Idle);
        assert_eq!(controller.recording_started_at_ms.get(), None);
        assert!(!controller.transcription.is_transcribing.get());
        assert_eq!(controller.feedback_message.get(), None);
        assert_eq!(
            controller.error_message.get().as_deref(),
            Some("Live transcription failed: provider mismatch")
        );
        assert_eq!(controller.transcription.transcript.get(), None);
        assert_eq!(
            controller.transcription.job_status.get(),
            crate::tauri_api::TranscriptionJobStatus {
                state: TranscriptionJobState::Failed,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("provider mismatch".to_string()),
            }
        );
        assert_eq!(
            controller.last_result.get(),
            Some(LiveRecordingSummary {
                input_device_label: "Desk Mic".to_string(),
                sample_rate_hz: 48_000,
                channels: 2,
                duration_ms: 5_200,
            })
        );
    }

    #[test]
    fn live_transcription_source_name_prefers_trimmed_device_label() {
        let source_name = live_transcription_source_name(&LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some("mic-1".to_string()),
            input_device_label: "  Desk Mic  ".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
        });

        assert_eq!(source_name, "Desk Mic");
    }

    #[test]
    fn live_transcription_source_name_falls_back_to_device_id_then_default() {
        let source_name = live_transcription_source_name(&LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some(" mic-1 ".to_string()),
            input_device_label: "   ".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
        });
        assert_eq!(source_name, "mic-1");

        let fallback_name = live_transcription_source_name(&LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: None,
            input_device_label: String::new(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
        });
        assert_eq!(fallback_name, "Live recording");
    }

    fn recording_status(input_device_label: &str) -> LiveRecordingStatus {
        LiveRecordingStatus {
            state: LiveRecordingState::Recording,
            input_device_id: Some("mic-1".to_string()),
            input_device_label: Some(input_device_label.to_string()),
            output_file_path: Some("/tmp/capture.wav".to_string()),
            sample_rate_hz: Some(48_000),
            channels: Some(2),
            duration_ms: Some(5_200),
            message: None,
        }
    }

    fn sample_capture_result() -> LiveRecordingResult {
        LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some("mic-1".to_string()),
            input_device_label: "Desk Mic".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
        }
    }

    fn sample_transcript_result(source_name: &str, text: &str) -> TranscriptResult {
        TranscriptResult {
            text: text.to_string(),
            segments: vec![TranscriptSegment {
                start_ms: 0,
                end_ms: 1_200,
                text: text.to_string(),
            }],
            source: TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::Live,
                source_name: Some(source_name.to_string()),
                duration_ms: Some(5_200),
            },
            post_processed_text: None,
        }
    }
}
