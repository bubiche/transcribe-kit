#[cfg(target_arch = "wasm32")]
use js_sys::Date;

use crate::tauri_api::{LiveRecordingState, LiveRecordingStatus};

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

pub(super) fn idle_status() -> LiveRecordingStatus {
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

pub(super) fn now_ms() -> f64 {
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
