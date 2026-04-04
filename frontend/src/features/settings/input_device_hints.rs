use crate::tauri_api::AudioInputDeviceDescriptor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputDeviceKindHint {
    PhysicalMic,
    VirtualLoopback,
    MonitorSource,
    Unknown,
}

impl InputDeviceKindHint {
    pub(super) fn badge_label(self) -> &'static str {
        match self {
            Self::PhysicalMic => "Mic",
            Self::VirtualLoopback => "Loopback",
            Self::MonitorSource => "Monitor",
            Self::Unknown => "Unknown",
        }
    }

    pub(super) fn class_name(self) -> &'static str {
        match self {
            Self::PhysicalMic => "mic",
            Self::VirtualLoopback => "loopback",
            Self::MonitorSource => "monitor",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) fn classify_input_device(device: &AudioInputDeviceDescriptor) -> InputDeviceKindHint {
    let label = device.label.to_lowercase();
    let manufacturer = device
        .manufacturer
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let combined = format!("{label} {manufacturer}");

    if contains_any(&combined, &["monitor", "what u hear", "wave out"]) {
        return InputDeviceKindHint::MonitorSource;
    }

    if contains_any(
        &combined,
        &[
            "loopback",
            "stereo mix",
            "virtual cable",
            "blackhole",
            "soundflower",
            "vb-audio",
            "aggregate",
            "rogue amoeba",
            "existential audio",
        ],
    ) || (combined.contains("virtual") && combined.contains("cable"))
    {
        return InputDeviceKindHint::VirtualLoopback;
    }

    if contains_any(
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
    ) {
        return InputDeviceKindHint::PhysicalMic;
    }

    InputDeviceKindHint::Unknown
}

pub(super) fn contains_any(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

pub(super) fn format_input_device_option_label(device: &AudioInputDeviceDescriptor) -> String {
    let default_suffix = if device.is_default { " (Default)" } else { "" };
    format!(
        "[{}] {}{}",
        classify_input_device(device).badge_label(),
        device.label,
        default_suffix
    )
}

pub(super) fn system_default_option_label(devices: &[AudioInputDeviceDescriptor]) -> String {
    match effective_input_device(None, devices) {
        Some(device) => format!(
            "System default input (currently [{}] {})",
            classify_input_device(device).badge_label(),
            device.label
        ),
        None => "System default input".to_string(),
    }
}

pub(super) fn input_device_kind_summary(kind: InputDeviceKindHint) -> &'static str {
    match kind {
        InputDeviceKindHint::PhysicalMic => "Detected as a mic-style input.",
        InputDeviceKindHint::VirtualLoopback => "Detected as a loopback or virtual input.",
        InputDeviceKindHint::MonitorSource => "Detected as a monitor-style input.",
        InputDeviceKindHint::Unknown => "The input type is not obvious from the device name.",
    }
}

pub(super) fn effective_input_device<'a>(
    selected_input_device_id: Option<&str>,
    devices: &'a [AudioInputDeviceDescriptor],
) -> Option<&'a AudioInputDeviceDescriptor> {
    match selected_input_device_id {
        Some(selected_id) => devices.iter().find(|device| device.id == selected_id),
        None => devices.iter().find(|device| device.is_default),
    }
}

pub(super) fn describe_input_device(device: &AudioInputDeviceDescriptor) -> String {
    let mut details = Vec::new();

    if let Some(manufacturer) = device.manufacturer.as_deref() {
        details.push(manufacturer.to_string());
    }

    if let Some(channels) = device.channels {
        details.push(format!("{channels} ch"));
    }

    if let Some(sample_rate_hz) = device.sample_rate_hz {
        details.push(format!("{sample_rate_hz} Hz"));
    }

    if details.is_empty() {
        format!(
            "{} is ready for live recording. {}",
            device.label,
            input_device_kind_summary(classify_input_device(device))
        )
    } else {
        format!(
            "{}: {}. {}",
            device.label,
            details.join(" • "),
            input_device_kind_summary(classify_input_device(device))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device(label: &str) -> AudioInputDeviceDescriptor {
        AudioInputDeviceDescriptor {
            id: label.to_string(),
            label: label.to_string(),
            manufacturer: None,
            channels: Some(2),
            sample_rate_hz: Some(48_000),
            is_default: false,
        }
    }

    #[test]
    fn classifies_loopback_devices_from_label() {
        let device = sample_device("BlackHole 2ch");
        assert_eq!(
            classify_input_device(&device),
            InputDeviceKindHint::VirtualLoopback
        );
    }

    #[test]
    fn classifies_monitor_devices_from_label() {
        let device = sample_device("Monitor of Built-in Audio");
        assert_eq!(
            classify_input_device(&device),
            InputDeviceKindHint::MonitorSource
        );
    }

    #[test]
    fn classifies_microphones_from_label() {
        let device = sample_device("MacBook Pro Microphone");
        assert_eq!(
            classify_input_device(&device),
            InputDeviceKindHint::PhysicalMic
        );
    }

    #[test]
    fn picker_option_labels_include_device_badges() {
        let device = sample_device("BlackHole 2ch");
        assert_eq!(
            format_input_device_option_label(&device),
            "[Loopback] BlackHole 2ch"
        );
    }

    #[test]
    fn system_default_option_shows_current_default_device_kind() {
        let mut device = sample_device("BlackHole 2ch");
        device.is_default = true;

        assert_eq!(
            system_default_option_label(&[device]),
            "System default input (currently [Loopback] BlackHole 2ch)"
        );
    }
}
