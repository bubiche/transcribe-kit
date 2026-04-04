use cpal::traits::{DeviceTrait, HostTrait};

use crate::models::AudioInputDeviceDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum InputDeviceError {
    #[error("Transcribe Kit could not enumerate audio input devices: {0}")]
    EnumerateDevices(#[source] cpal::DevicesError),
}

pub fn list_input_devices() -> Result<Vec<AudioInputDeviceDescriptor>, InputDeviceError> {
    let host = cpal::default_host();
    let default_device_id = host
        .default_input_device()
        .and_then(|device| device.id().ok().map(|device_id| device_id.to_string()));

    let mut devices = host
        .input_devices()
        .map_err(InputDeviceError::EnumerateDevices)?
        .filter_map(|device| {
            let device_id = device.id().ok()?.to_string();
            let description = device.description().ok();
            let default_config = device.default_input_config().ok();

            let label = description
                .as_ref()
                .map(|description| description.name().to_string())
                .unwrap_or_else(|| format!("Audio input {}", &device_id[..8.min(device_id.len())]));

            Some(AudioInputDeviceDescriptor {
                id: device_id.clone(),
                label,
                manufacturer: description
                    .as_ref()
                    .and_then(|description| description.manufacturer().map(str::to_string)),
                channels: default_config.as_ref().map(|config| config.channels()),
                sample_rate_hz: default_config.as_ref().map(|config| config.sample_rate()),
                is_default: default_device_id.as_deref() == Some(device_id.as_str()),
            })
        })
        .collect::<Vec<_>>();

    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(devices)
}
