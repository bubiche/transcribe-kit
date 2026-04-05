use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

mod device_context;
mod recording_goal;
#[cfg(test)]
mod tests;
mod timing;
mod transcription_flow;

use self::device_context::{is_armed_for_dual_capture, resolve_armed_input_label};
use self::recording_goal::{desired_recording_goal, next_command_for_goal, LiveRecordingCommand};
pub use self::timing::{format_duration, live_elapsed_duration_ms};
use self::timing::{idle_status, now_ms};
use self::transcription_flow::{
    apply_live_transcription_failed, apply_live_transcription_started,
    apply_live_transcription_succeeded, apply_stop_ui_transition, live_transcription_source_name,
    stop_ui_transition,
};

use crate::features::transcription::TranscriptionController;
use crate::tauri_api::{
    get_live_recording_status, get_settings, list_input_devices, listen_to_app_event,
    start_live_transcription, stop_live_transcription, transcribe_live_recording,
    HotkeyActivityEvent, LiveCaptureProfile, LiveRecordingResult, LiveRecordingState,
    LiveRecordingStatus, TranscribeLiveRecordingRequest,
};

pub const HOTKEY_ACTIVITY_EVENT_NAME: &str = "transcribe-kit://live-recording-hotkey";
pub const LIVE_RECORDING_STATUS_EVENT_NAME: &str = "transcribe-kit://live-recording-status";

#[derive(Clone, Copy)]
pub struct LiveRecordingController {
    pub status: RwSignal<LiveRecordingStatus>,
    pub armed_input_label: RwSignal<String>,
    pub armed_capture_profile: RwSignal<LiveCaptureProfile>,
    pub armed_dual_capture: RwSignal<bool>,
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
            armed_capture_profile: RwSignal::new(LiveCaptureProfile::default()),
            armed_dual_capture: RwSignal::new(false),
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
                    self.armed_capture_profile
                        .set(settings.live_capture_profile);
                    self.armed_dual_capture
                        .set(is_armed_for_dual_capture(&settings, &devices));
                    self.armed_input_label
                        .set(resolve_armed_input_label(&settings, &devices));
                    self.device_context_error.set(None);
                }
                (Err(settings_error), Err(devices_error)) => {
                    self.armed_dual_capture.set(false);
                    self.device_context_error.set(Some(format!(
                        "Saved recording device context could not be refreshed: settings: {settings_error} | input devices: {devices_error}"
                    )));
                }
                (Err(error), _) => {
                    self.armed_dual_capture.set(false);
                    self.device_context_error.set(Some(format!(
                        "Saved recording settings could not be refreshed: {error}"
                    )));
                }
                (_, Err(error)) => {
                    self.armed_dual_capture.set(false);
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
                        let expected_dual_capture = controller.armed_dual_capture.get_untracked()
                            && matches!(
                                controller.armed_capture_profile.get_untracked(),
                                LiveCaptureProfile::MeetingMix
                            );
                        let transition =
                            stop_ui_transition(Ok(result.clone()), expected_dual_capture);
                        apply_stop_ui_transition(controller, &transition);

                        if transition.armed_input_label.is_some() {
                            controller.refresh_armed_device_context();
                        }

                        run_live_transcription(controller, result).await;
                    }
                    Err(error) => {
                        let transition = stop_ui_transition(Err(error), false);
                        apply_stop_ui_transition(controller, &transition);
                    }
                }

                controller.request_in_flight.set(false);
                reconcile_recording_goal(controller);
            });
        }
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
            live_capture_profile: controller.armed_capture_profile.get_untracked(),
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
