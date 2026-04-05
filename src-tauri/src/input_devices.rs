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
                is_output_loopback: false,
            })
        })
        .collect::<Vec<_>>();

    if platform_supports_output_loopback() {
        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                let device_id = match device.id() {
                    Ok(device_id) => device_id.to_string(),
                    Err(_) => continue,
                };
                let description = device.description().ok();
                let default_config = device.default_output_config().ok();
                let label = description
                    .as_ref()
                    .map(|description| format!("{} (System Audio)", description.name()))
                    .unwrap_or_else(|| {
                        format!("System Audio {}", &device_id[..8.min(device_id.len())])
                    });

                devices.push(AudioInputDeviceDescriptor {
                    id: device_id,
                    label,
                    manufacturer: description
                        .as_ref()
                        .and_then(|description| description.manufacturer().map(str::to_string)),
                    channels: default_config.as_ref().map(|config| config.channels()),
                    sample_rate_hz: default_config.as_ref().map(|config| config.sample_rate()),
                    is_default: false,
                    is_output_loopback: true,
                });
            }
        }
    }

    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(devices)
}

fn platform_supports_output_loopback() -> bool {
    #[cfg(target_os = "windows")]
    {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        return macos_version_at_least(14, 2);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn macos_version_at_least(major: u32, minor: u32) -> bool {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|version| parse_macos_version(version.trim()))
        .map(|(actual_major, actual_minor, _)| {
            (actual_major, actual_minor) >= (major, minor)
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn parse_macos_version(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(str::trim);
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}
