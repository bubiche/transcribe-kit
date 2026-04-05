use crate::tauri_api::{AudioInputDeviceDescriptor, LiveCaptureProfile};

use super::input_device_hints::{
    classify_input_device, contains_any, effective_input_device, InputDeviceKindHint,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MeetingReadinessTone {
    Good,
    Caution,
    Warning,
}

impl MeetingReadinessTone {
    pub(super) fn class_name(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Caution => "caution",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MeetingReadinessHint {
    pub title: String,
    pub body: String,
    pub tone: MeetingReadinessTone,
    pub device_kind: Option<InputDeviceKindHint>,
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimePlatform {
    MacOS,
    Windows,
    Linux,
    Unknown,
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
impl RuntimePlatform {
    fn detect(platform_hint: &str, user_agent: &str) -> Self {
        let combined = format!(
            "{} {}",
            platform_hint.to_lowercase(),
            user_agent.to_lowercase()
        );

        if contains_any(&combined, &["mac", "darwin", "os x"]) {
            return Self::MacOS;
        }

        if combined.contains("win") {
            return Self::Windows;
        }

        if contains_any(&combined, &["linux", "x11"]) {
            return Self::Linux;
        }

        Self::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CaptureBehaviorSummary {
    pub title: String,
    pub body: String,
    pub tone: MeetingReadinessTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlatformMeetingGuidance {
    pub platform_label: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

#[cfg(target_arch = "wasm32")]
pub(super) fn detect_runtime_platform() -> RuntimePlatform {
    let Some(window) = web_sys::window() else {
        return RuntimePlatform::Unknown;
    };
    let navigator = window.navigator();
    let platform_hint = navigator.platform().unwrap_or_default();

    RuntimePlatform::detect(
        platform_hint.as_str(),
        &navigator.user_agent().unwrap_or_default(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn detect_runtime_platform() -> RuntimePlatform {
    RuntimePlatform::Unknown
}

pub(super) fn platform_meeting_guidance(platform: RuntimePlatform) -> PlatformMeetingGuidance {
    match platform {
        RuntimePlatform::MacOS => PlatformMeetingGuidance {
            platform_label: "macOS",
            title: "Meeting mix on macOS supports automatic dual capture.",
            body: "Transcribe Kit automatically captures both your microphone and system audio when Meeting mix is selected with a microphone input. No extra setup is needed on macOS 14.2+.",
        },
        RuntimePlatform::Windows => PlatformMeetingGuidance {
            platform_label: "Windows",
            title: "Meeting mix on Windows supports automatic dual capture.",
            body: "Transcribe Kit automatically captures both your microphone and system audio when Meeting mix is selected with a microphone input.",
        },
        RuntimePlatform::Linux => PlatformMeetingGuidance {
            platform_label: "Linux",
            title: "Meeting capture on Linux usually relies on monitor sources or virtual inputs.",
            body: "Look for a PipeWire or PulseAudio monitor source, or another mixed input exposed by the OS. Transcribe Kit records that input directly once you select it.",
        },
        RuntimePlatform::Unknown => PlatformMeetingGuidance {
            platform_label: "This device",
            title: "Meeting capture works best with a mixed input exposed by the operating system.",
            body: "Look for a System Audio source, or a loopback, monitor, Stereo Mix, or virtual cable style input. Transcribe Kit records whichever audio source you select.",
        },
    }
}

pub(super) fn dual_capture_available(devices: &[AudioInputDeviceDescriptor]) -> bool {
    devices.iter().any(|device| device.is_output_loopback)
}

pub(super) fn build_meeting_readiness_hint(
    profile: LiveCaptureProfile,
    selected_input_device_id: Option<&str>,
    devices: &[AudioInputDeviceDescriptor],
    dual_capture_available: bool,
) -> MeetingReadinessHint {
    if devices.is_empty() {
        return MeetingReadinessHint {
            title: "No: no audio input is available right now.".to_string(),
            body: "Transcribe Kit cannot capture a meeting until the OS exposes at least one audio input. Reconnect a device or keep System default selected and try again after the input appears.".to_string(),
            tone: MeetingReadinessTone::Warning,
            device_kind: None,
        };
    }

    if let Some(selected_id) = selected_input_device_id {
        if !devices.iter().any(|device| device.id == selected_id) {
            return MeetingReadinessHint {
                title: "No: the selected audio input is no longer available.".to_string(),
                body: "Choose another input or switch back to System default before starting a meeting capture so the app records from a real source.".to_string(),
                tone: MeetingReadinessTone::Warning,
                device_kind: None,
            };
        }
    }

    let effective_device = effective_input_device(selected_input_device_id, devices);
    let device_kind = effective_device.map(classify_input_device);

    match (profile, device_kind) {
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::SystemAudio)) => {
            MeetingReadinessHint {
                title: "Partial: this captures remote participants but not your own voice.".to_string(),
                body: "System Audio records what plays through the output device — typically the other side of the call. Your own voice goes through your microphone and is not included. For both sides, use a loopback or virtual cable input that mixes mic and speaker audio.".to_string(),
                tone: MeetingReadinessTone::Caution,
                device_kind,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::VirtualLoopback)) => {
            MeetingReadinessHint {
                title: "Yes: this looks like a strong meeting-mix input.".to_string(),
                body: "Loopback and virtual cable inputs usually contain both local and remote audio before Transcribe Kit starts recording.".to_string(),
                tone: MeetingReadinessTone::Good,
                device_kind,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::MonitorSource)) => {
            MeetingReadinessHint {
                title: "Yes: this monitor-style input is a good meeting candidate.".to_string(),
                body: "Monitor sources often expose the mixed output you need for full meeting capture. Run a short test recording if you want to confirm levels.".to_string(),
                tone: MeetingReadinessTone::Good,
                device_kind,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::PhysicalMic)) => {
            if dual_capture_available {
                MeetingReadinessHint {
                    title: "Yes: both sides of the meeting will be captured.".to_string(),
                    body: "Transcribe Kit will record your microphone and system audio simultaneously, then combine them for transcription.".to_string(),
                    tone: MeetingReadinessTone::Good,
                    device_kind,
                }
            } else {
                MeetingReadinessHint {
                    title: "No: this still looks like a microphone.".to_string(),
                    body: "Meeting mix works best with an input that already combines your microphone and the call audio. A plain mic will usually miss remote participants.".to_string(),
                    tone: MeetingReadinessTone::Warning,
                    device_kind,
                }
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::Unknown))
        | (LiveCaptureProfile::MeetingMix, None) => MeetingReadinessHint {
            title: "Maybe: this input might work, but it is not clearly a mixed source."
                .to_string(),
            body: "For full meeting capture, prefer a loopback, monitor, or virtual cable input. If you stay on this source, do a quick test before relying on it.".to_string(),
            tone: MeetingReadinessTone::Caution,
            device_kind,
        },
        (LiveCaptureProfile::MicrophoneOnly, Some(InputDeviceKindHint::PhysicalMic)) => {
            MeetingReadinessHint {
                title: "No: this setup is aimed at your voice, not the full meeting.".to_string(),
                body: "Microphone-only capture is the right choice for dictation or personal notes. Switch to Meeting mix if you want to include remote speakers.".to_string(),
                tone: MeetingReadinessTone::Warning,
                device_kind,
            }
        }
        (
            LiveCaptureProfile::MicrophoneOnly,
            Some(
                InputDeviceKindHint::VirtualLoopback
                    | InputDeviceKindHint::MonitorSource
                    | InputDeviceKindHint::SystemAudio,
            ),
        ) => MeetingReadinessHint {
            title: "Maybe: the device could capture the meeting, but the profile is still set to microphone only.".to_string(),
            body: "Recording will still use this input, but switching the profile to Meeting mix makes the capture intent clearer and keeps future labeling honest.".to_string(),
            tone: MeetingReadinessTone::Caution,
            device_kind,
        },
        (LiveCaptureProfile::MicrophoneOnly, Some(InputDeviceKindHint::Unknown))
        | (LiveCaptureProfile::MicrophoneOnly, None) => MeetingReadinessHint {
            title: "Maybe: this could capture only part of the meeting.".to_string(),
            body: "If the goal is full meeting capture, switch to Meeting mix and choose an input that clearly looks like a loopback, monitor, or virtual cable source.".to_string(),
            tone: MeetingReadinessTone::Caution,
            device_kind,
        },
    }
}

pub(super) fn build_capture_behavior_summary(
    profile: LiveCaptureProfile,
    selected_input_device_id: Option<&str>,
    devices: &[AudioInputDeviceDescriptor],
    dual_capture_available: bool,
) -> CaptureBehaviorSummary {
    if devices.is_empty() {
        return CaptureBehaviorSummary {
            title: "Not ready: no audio input is available.".to_string(),
            body: "Live capture cannot start until the OS exposes at least one audio input. Once an input appears, Transcribe Kit will record whichever source you choose here.".to_string(),
            tone: MeetingReadinessTone::Warning,
        };
    }

    if let Some(selected_id) = selected_input_device_id {
        if !devices.iter().any(|device| device.id == selected_id) {
            return CaptureBehaviorSummary {
                title: "Not ready: the saved audio input is unavailable.".to_string(),
                body: "Pick another source or switch back to System default before recording so the app captures from a real audio input.".to_string(),
                tone: MeetingReadinessTone::Warning,
            };
        }
    }

    let effective_device = effective_input_device(selected_input_device_id, devices);
    let device_label = effective_device
        .map(|device| device.label.as_str())
        .unwrap_or("System default input");
    let device_kind = effective_device.map(classify_input_device);

    match (profile, device_kind) {
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::SystemAudio)) => {
            CaptureBehaviorSummary {
                title: format!("Ready to record remote audio from {device_label}."),
                body: "System Audio captures what plays through this output — typically the remote side of a call. Your own voice is not included because it goes through your microphone, not your speakers.".to_string(),
                tone: MeetingReadinessTone::Caution,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::VirtualLoopback)) => {
            CaptureBehaviorSummary {
                title: format!("Ready to record from {device_label} as a meeting mix."),
                body: "This input looks like a loopback or virtual source, so it will likely capture both your microphone and remote speakers if your routing is already set up.".to_string(),
                tone: MeetingReadinessTone::Good,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::MonitorSource)) => {
            CaptureBehaviorSummary {
                title: format!("Ready to record from {device_label} as a meeting mix."),
                body: "This monitor-style input often carries the mixed meeting audio Transcribe Kit needs for both sides of the conversation.".to_string(),
                tone: MeetingReadinessTone::Good,
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::PhysicalMic)) => {
            if dual_capture_available {
                CaptureBehaviorSummary {
                    title: format!(
                        "Ready to record both sides from {device_label} + system audio."
                    ),
                    body: "Your voice will be captured from the selected input, and remote participants will be captured from system audio output. Both are mixed into one transcript.".to_string(),
                    tone: MeetingReadinessTone::Good,
                }
            } else {
                CaptureBehaviorSummary {
                    title: format!(
                        "Ready to record from {device_label}, but meeting capture may be incomplete."
                    ),
                    body: "Transcribe Kit will record this microphone exactly as selected, which usually means the transcript will focus on your voice and may miss remote speakers.".to_string(),
                    tone: MeetingReadinessTone::Warning,
                }
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::Unknown))
        | (LiveCaptureProfile::MeetingMix, None) => CaptureBehaviorSummary {
            title: format!("Ready to record from {device_label}, but test this setup first."),
            body: "The chosen source is not clearly a mixed input, so run a short sample recording before relying on it for a real meeting.".to_string(),
            tone: MeetingReadinessTone::Caution,
        },
        (LiveCaptureProfile::MicrophoneOnly, Some(InputDeviceKindHint::PhysicalMic)) => {
            CaptureBehaviorSummary {
                title: format!("Ready to record from {device_label}."),
                body: "Microphone-only capture is best for your own voice, dictation, and notes. Transcribe Kit will record this input source directly.".to_string(),
                tone: MeetingReadinessTone::Good,
            }
        }
        (
            LiveCaptureProfile::MicrophoneOnly,
            Some(
                InputDeviceKindHint::VirtualLoopback
                    | InputDeviceKindHint::MonitorSource
                    | InputDeviceKindHint::SystemAudio,
            ),
        ) => CaptureBehaviorSummary {
            title: format!("Ready to record from {device_label}."),
            body: "Because this input looks mixed, it may include the whole meeting even though the profile is still set to Microphone only. Switch to Meeting mix if that is your intent.".to_string(),
            tone: MeetingReadinessTone::Caution,
        },
        (LiveCaptureProfile::MicrophoneOnly, Some(InputDeviceKindHint::Unknown))
        | (LiveCaptureProfile::MicrophoneOnly, None) => CaptureBehaviorSummary {
            title: format!("Ready to record from {device_label}."),
            body: "Transcribe Kit will record this selected input source as-is. If you want the full meeting instead of mostly your own voice, switch to Meeting mix first.".to_string(),
            tone: MeetingReadinessTone::Caution,
        },
    }
}

pub(super) fn build_meeting_troubleshooting_steps(
    platform: RuntimePlatform,
    profile: LiveCaptureProfile,
    selected_input_device_id: Option<&str>,
    devices: &[AudioInputDeviceDescriptor],
    dual_capture_available: bool,
) -> Vec<String> {
    let effective_kind =
        effective_input_device(selected_input_device_id, devices).map(classify_input_device);
    let mut steps = vec![
        "Run a 10-second test recording after changing audio routing, default inputs, or meeting-app audio settings.".to_string(),
    ];

    match (profile, effective_kind) {
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::PhysicalMic)) => {
            if dual_capture_available {
                steps.insert(
                    0,
                    "Meeting mix with a microphone uses dual capture automatically. If remote voices are still missing, check system audio capture permissions and run another short test."
                        .to_string(),
                )
            } else {
                steps.insert(
                    0,
                    "If the transcript mostly contains your own voice, switch away from a plain microphone and choose a loopback, monitor, or virtual cable input instead."
                        .to_string(),
                )
            }
        }
        (LiveCaptureProfile::MeetingMix, Some(InputDeviceKindHint::Unknown))
        | (LiveCaptureProfile::MeetingMix, None) => steps.insert(
            0,
            "If you are unsure about this source, prefer an input whose name clearly suggests loopback, monitor, Stereo Mix, or virtual cable behavior.".to_string(),
        ),
        _ => steps.insert(
            0,
            "Transcribe Kit records one selected audio input at a time, so the input itself needs to contain the audio you want before recording begins.".to_string(),
        ),
    }

    steps.push(match platform {
        RuntimePlatform::MacOS => {
            "On macOS 14.2+, dual capture requires System Audio Recording permission. Grant it in System Settings > Privacy & Security > Screen & System Audio Recording, then restart the app.".to_string()
        }
        RuntimePlatform::Windows => {
            "On Windows, System Audio outputs should appear automatically for loopback capture. If you prefer a different setup, check whether Stereo Mix, a loopback device, or a virtual cable input can be enabled in your audio settings.".to_string()
        }
        RuntimePlatform::Linux => {
            "If no meeting-style input appears on Linux, expose a monitor source or virtual input through PipeWire or PulseAudio, then refresh your device list.".to_string()
        }
        RuntimePlatform::Unknown => {
            "If no meeting-style input appears, enable or create a mixed input in your operating system or audio-routing tool before trying again.".to_string()
        }
    });

    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tauri_api::AudioInputDeviceDescriptor;

    fn sample_device(label: &str) -> AudioInputDeviceDescriptor {
        AudioInputDeviceDescriptor {
            id: label.to_string(),
            label: label.to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
            is_output_loopback: false,
        }
    }

    #[test]
    fn meeting_mix_hint_recommends_loopback_inputs() {
        let device = sample_device("BlackHole 2ch");
        let selected_id = device.id.clone();
        let hint = build_meeting_readiness_hint(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            false,
        );

        assert_eq!(hint.tone, MeetingReadinessTone::Good);
        assert!(hint.title.starts_with("Yes:"));
    }

    #[test]
    fn meeting_mix_hint_warns_for_microphones() {
        let device = sample_device("USB Microphone");
        let selected_id = device.id.clone();
        let hint = build_meeting_readiness_hint(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            false,
        );

        assert_eq!(hint.tone, MeetingReadinessTone::Warning);
        assert!(hint.title.starts_with("No:"));
    }

    #[test]
    fn meeting_mix_hint_cautions_system_audio_captures_only_remote() {
        let mut device = sample_device("Built-in Output (System Audio)");
        device.is_output_loopback = true;
        let selected_id = device.id.clone();
        let hint = build_meeting_readiness_hint(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            true,
        );

        assert_eq!(hint.tone, MeetingReadinessTone::Caution);
        assert!(hint.title.starts_with("Partial:"));
        assert!(hint.body.contains("not included"));
    }

    #[test]
    fn meeting_readiness_warns_when_selected_device_is_missing() {
        let hint = build_meeting_readiness_hint(
            LiveCaptureProfile::MeetingMix,
            Some("missing-device"),
            &[sample_device("BlackHole 2ch")],
            false,
        );

        assert_eq!(hint.tone, MeetingReadinessTone::Warning);
        assert!(hint.title.contains("no longer available"));
    }

    #[test]
    fn meeting_readiness_warns_when_no_devices_are_available() {
        let hint = build_meeting_readiness_hint(LiveCaptureProfile::MeetingMix, None, &[], false);

        assert_eq!(hint.tone, MeetingReadinessTone::Warning);
        assert!(hint.title.contains("no audio input"));
    }

    #[test]
    fn detects_runtime_platforms_from_platform_and_user_agent_hints() {
        assert_eq!(
            RuntimePlatform::detect("MacIntel", "Mozilla/5.0"),
            RuntimePlatform::MacOS
        );
        assert_eq!(
            RuntimePlatform::detect("Win32", "Mozilla/5.0"),
            RuntimePlatform::Windows
        );
        assert_eq!(
            RuntimePlatform::detect("x86_64", "Mozilla/5.0 (X11; Linux x86_64)"),
            RuntimePlatform::Linux
        );
    }

    #[test]
    fn capture_behavior_summary_warns_for_meeting_mix_on_plain_mic() {
        let device = sample_device("USB Microphone");
        let selected_id = device.id.clone();
        let summary = build_capture_behavior_summary(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            false,
        );

        assert_eq!(summary.tone, MeetingReadinessTone::Warning);
        assert!(summary.title.contains("may be incomplete"));
    }

    #[test]
    fn capture_behavior_summary_confirms_loopback_meeting_mix() {
        let device = sample_device("BlackHole 2ch");
        let selected_id = device.id.clone();
        let summary = build_capture_behavior_summary(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            false,
        );

        assert_eq!(summary.tone, MeetingReadinessTone::Good);
        assert!(summary.title.contains("meeting mix"));
    }

    #[test]
    fn troubleshooting_steps_call_out_plain_mic_meeting_mix() {
        let device = sample_device("USB Microphone");
        let selected_id = device.id.clone();
        let steps = build_meeting_troubleshooting_steps(
            RuntimePlatform::Windows,
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &[device],
            false,
        );

        assert!(steps[0].contains("mostly contains your own voice"));
        assert!(steps[2].contains("Windows"));
    }

    #[test]
    fn dual_capture_available_is_true_when_system_audio_device_exists() {
        let mut system_audio = sample_device("Built-in Output");
        system_audio.is_output_loopback = true;

        assert!(dual_capture_available(&[
            sample_device("USB Microphone"),
            system_audio
        ]));
        assert!(!dual_capture_available(&[sample_device("USB Microphone")]));
    }

    #[test]
    fn meeting_mix_hint_confirms_dual_capture_for_microphone_when_available() {
        let mic = sample_device("USB Microphone");
        let selected_id = mic.id.clone();
        let mut system_audio = sample_device("Built-in Output");
        system_audio.is_output_loopback = true;
        let devices = vec![mic, system_audio];
        let hint = build_meeting_readiness_hint(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &devices,
            dual_capture_available(&devices),
        );

        assert_eq!(hint.tone, MeetingReadinessTone::Good);
        assert!(hint.body.contains("microphone and system audio"));
    }

    #[test]
    fn capture_behavior_summary_mentions_mic_and_system_audio_when_dual_capture_ready() {
        let mic = sample_device("USB Microphone");
        let selected_id = mic.id.clone();
        let mut system_audio = sample_device("Built-in Output");
        system_audio.is_output_loopback = true;
        let devices = vec![mic, system_audio];
        let summary = build_capture_behavior_summary(
            LiveCaptureProfile::MeetingMix,
            Some(selected_id.as_str()),
            &devices,
            dual_capture_available(&devices),
        );

        assert_eq!(summary.tone, MeetingReadinessTone::Good);
        assert!(summary.title.contains("+ system audio"));
        assert!(summary.body.contains("selected input"));
    }
}
