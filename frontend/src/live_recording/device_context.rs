use crate::tauri_api::{AppSettings, AudioInputDeviceDescriptor, LiveCaptureProfile};

pub(super) fn resolve_armed_input_label(
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

pub(super) fn is_armed_for_dual_capture(
    settings: &AppSettings,
    devices: &[AudioInputDeviceDescriptor],
) -> bool {
    if !matches!(
        settings.live_capture_profile,
        LiveCaptureProfile::MeetingMix
    ) {
        return false;
    }

    if !has_output_loopback_device(devices) {
        return false;
    }

    let effective_device = match settings.selected_input_device_id.as_deref() {
        Some(selected_id) => devices.iter().find(|device| device.id == selected_id),
        None => devices.iter().find(|device| device.is_default),
    };

    effective_device.map(is_mic_like_input).unwrap_or(false)
}

fn has_output_loopback_device(devices: &[AudioInputDeviceDescriptor]) -> bool {
    devices.iter().any(|device| device.is_output_loopback)
}

fn contains_any(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

fn is_mic_like_input(device: &AudioInputDeviceDescriptor) -> bool {
    if device.is_output_loopback {
        return false;
    }

    let combined = format!(
        "{} {}",
        device.label.to_lowercase(),
        device
            .manufacturer
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
    );

    contains_any(
        &combined,
        &[
            "microphone",
            "mic",
            "headset",
            "airpods",
            "webcam",
            "built-in",
            "array",
            "internal mic",
        ],
    )
}
