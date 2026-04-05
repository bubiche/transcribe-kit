use super::*;
use crate::tauri_api::{
    AppSettings, AudioInputDeviceDescriptor, HotkeyActivityState, HotkeyMode, InputType,
    LiveCaptureProfile, LiveRecordingResult, LiveRecordingState, LiveRecordingStatus,
    TranscriptResult, TranscriptSegment, TranscriptionJobState, TranscriptionSource,
};

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
    let goal = desired_recording_goal(HotkeyMode::Toggle, HotkeyActivityState::Pressed, true, true);

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
    let transition = stop_ui_transition(Err("writer finalize failed".to_string()), false);

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
    let transition = stop_ui_transition(
        Ok(LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some("mic-1".to_string()),
            input_device_label: "Desk Mic".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
            is_dual_capture: false,
        }),
        false,
    );

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
fn stop_success_transition_warns_when_dual_capture_was_expected_but_not_used() {
    let transition = stop_ui_transition(
        Ok(LiveRecordingResult {
            file_path: "/tmp/capture.wav".to_string(),
            input_device_id: Some("mic-1".to_string()),
            input_device_label: "Desk Mic".to_string(),
            sample_rate_hz: 48_000,
            channels: 2,
            duration_ms: 5_200,
            is_dual_capture: false,
        }),
        true,
    );

    assert_eq!(transition.status.state, LiveRecordingState::Idle);
    assert_eq!(transition.error_message, None);
    assert!(transition
        .feedback_message
        .as_deref()
        .unwrap_or_default()
        .contains("fell back to microphone-only recording"));
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
    let transition = stop_ui_transition(Ok(capture_result), false);
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

    let transition = stop_ui_transition(Ok(sample_capture_result()), false);
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
        is_dual_capture: false,
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
        is_dual_capture: false,
    });
    assert_eq!(source_name, "mic-1");

    let fallback_name = live_transcription_source_name(&LiveRecordingResult {
        file_path: "/tmp/capture.wav".to_string(),
        input_device_id: None,
        input_device_label: String::new(),
        sample_rate_hz: 48_000,
        channels: 2,
        duration_ms: 5_200,
        is_dual_capture: false,
    });
    assert_eq!(fallback_name, "Live recording");
}

#[test]
fn armed_dual_capture_requires_meeting_mix_mic_and_system_audio_support() {
    let settings = AppSettings {
        live_capture_profile: LiveCaptureProfile::MeetingMix,
        selected_input_device_id: Some("mic".to_string()),
        ..AppSettings::default()
    };
    let devices = vec![
        AudioInputDeviceDescriptor {
            id: "mic".to_string(),
            label: "USB Microphone".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: true,
            is_output_loopback: false,
        },
        AudioInputDeviceDescriptor {
            id: "system-audio".to_string(),
            label: "Built-in Output (System Audio)".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
            is_output_loopback: true,
        },
    ];

    assert!(is_armed_for_dual_capture(&settings, &devices));
}

#[test]
fn armed_dual_capture_is_false_when_selected_input_is_system_audio() {
    let settings = AppSettings {
        live_capture_profile: LiveCaptureProfile::MeetingMix,
        selected_input_device_id: Some("system-audio".to_string()),
        ..AppSettings::default()
    };
    let devices = vec![
        AudioInputDeviceDescriptor {
            id: "mic".to_string(),
            label: "USB Microphone".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
            is_output_loopback: false,
        },
        AudioInputDeviceDescriptor {
            id: "system-audio".to_string(),
            label: "Built-in Output (System Audio)".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: true,
            is_output_loopback: true,
        },
    ];

    assert!(!is_armed_for_dual_capture(&settings, &devices));
}

#[test]
fn armed_dual_capture_is_false_without_output_loopback_devices() {
    let settings = AppSettings {
        live_capture_profile: LiveCaptureProfile::MeetingMix,
        selected_input_device_id: Some("mic".to_string()),
        ..AppSettings::default()
    };
    let devices = vec![AudioInputDeviceDescriptor {
        id: "mic".to_string(),
        label: "USB Microphone".to_string(),
        manufacturer: None,
        channels: Some(2),
        sample_rate_hz: Some(48_000),
        is_default: true,
        is_output_loopback: false,
    }];

    assert!(!is_armed_for_dual_capture(&settings, &devices));
}

#[test]
fn armed_dual_capture_is_false_for_virtual_loopback_inputs() {
    let settings = AppSettings {
        live_capture_profile: LiveCaptureProfile::MeetingMix,
        selected_input_device_id: Some("loopback".to_string()),
        ..AppSettings::default()
    };
    let devices = vec![
        AudioInputDeviceDescriptor {
            id: "loopback".to_string(),
            label: "BlackHole 2ch".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: true,
            is_output_loopback: false,
        },
        AudioInputDeviceDescriptor {
            id: "system-audio".to_string(),
            label: "Built-in Output (System Audio)".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
            is_output_loopback: true,
        },
    ];

    assert!(!is_armed_for_dual_capture(&settings, &devices));
}

#[test]
fn armed_dual_capture_is_false_for_unknown_non_mic_labels() {
    let settings = AppSettings {
        live_capture_profile: LiveCaptureProfile::MeetingMix,
        selected_input_device_id: Some("line-in".to_string()),
        ..AppSettings::default()
    };
    let devices = vec![
        AudioInputDeviceDescriptor {
            id: "line-in".to_string(),
            label: "Line In Port".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: true,
            is_output_loopback: false,
        },
        AudioInputDeviceDescriptor {
            id: "system-audio".to_string(),
            label: "Built-in Output (System Audio)".to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
            is_output_loopback: true,
        },
    ];

    assert!(!is_armed_for_dual_capture(&settings, &devices));
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
        is_dual_capture: false,
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
            live_capture_profile: Some(LiveCaptureProfile::MicrophoneOnly),
            source_name: Some(source_name.to_string()),
            duration_ms: Some(5_200),
        },
        post_processed_text: None,
    }
}
