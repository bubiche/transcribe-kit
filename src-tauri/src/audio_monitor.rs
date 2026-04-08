use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, Stream, StreamConfig, I24, U24};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::live_recording;

pub const AUDIO_LEVEL_EVENT_NAME: &str = "transcribe-kit://audio-level";

const EMIT_INTERVAL: Duration = Duration::from_millis(66);

#[derive(Clone, Serialize)]
pub struct AudioLevelEvent {
    pub rms: f32,
    pub peak: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioMonitorError {
    #[error("Could not find the selected audio input device.")]
    DeviceNotFound,
    #[error("Could not determine a usable capture format: {0}")]
    DefaultConfig(String),
    #[error("Unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("Could not build the audio monitor stream: {0}")]
    BuildStream(String),
    #[error("Could not start the audio monitor stream: {0}")]
    PlayStream(String),
}

struct ActiveMonitor {
    _stream: Stream,
    stop_flag: Arc<AtomicBool>,
    emitter_thread: Option<JoinHandle<()>>,
}

impl Drop for ActiveMonitor {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(thread) = self.emitter_thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Default)]
pub struct AudioMonitorState {
    inner: Arc<Mutex<Option<ActiveMonitor>>>,
}

impl AudioMonitorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        selected_device_id: Option<&str>,
    ) -> Result<(), AudioMonitorError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        // Stop any existing monitor first (handles device switching).
        guard.take();

        let resolved = live_recording::resolve_input_device(selected_device_id)
            .map_err(|_| AudioMonitorError::DeviceNotFound)?;

        let config = resolved
            .device
            .default_input_config()
            .map_err(|e| AudioMonitorError::DefaultConfig(e.to_string()))?;

        let sample_format = config.sample_format();
        let stream_config: StreamConfig = config.into();

        let rms_atomic = Arc::new(AtomicU32::new(0_f32.to_bits()));
        let peak_atomic = Arc::new(AtomicU32::new(0_f32.to_bits()));

        let stream = build_monitor_stream(
            &resolved.device,
            &stream_config,
            sample_format,
            Arc::clone(&rms_atomic),
            Arc::clone(&peak_atomic),
        )?;

        stream
            .play()
            .map_err(|e| AudioMonitorError::PlayStream(e.to_string()))?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let emitter_stop = Arc::clone(&stop_flag);
        let app_handle = app.clone();

        let emitter_thread = thread::spawn(move || {
            while !emitter_stop.load(Ordering::Relaxed) {
                thread::sleep(EMIT_INTERVAL);

                let rms = f32::from_bits(rms_atomic.load(Ordering::Relaxed));
                let peak = f32::from_bits(peak_atomic.load(Ordering::Relaxed));

                let _ = app_handle.emit(AUDIO_LEVEL_EVENT_NAME, AudioLevelEvent { rms, peak });
            }
        });

        *guard = Some(ActiveMonitor {
            _stream: stream,
            stop_flag,
            emitter_thread: Some(emitter_thread),
        });

        Ok(())
    }

    pub fn stop(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.take(); // Drop triggers cleanup via ActiveMonitor::drop
    }
}

fn build_monitor_stream(
    device: &Device,
    stream_config: &StreamConfig,
    sample_format: SampleFormat,
    rms_atomic: Arc<AtomicU32>,
    peak_atomic: Arc<AtomicU32>,
) -> Result<Stream, AudioMonitorError> {
    match sample_format {
        SampleFormat::I8 => {
            build_monitor_typed_stream::<i8>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::I16 => {
            build_monitor_typed_stream::<i16>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::I24 => {
            build_monitor_typed_stream::<I24>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::I32 => {
            build_monitor_typed_stream::<i32>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::I64 => {
            build_monitor_typed_stream::<i64>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::U8 => {
            build_monitor_typed_stream::<u8>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::U16 => {
            build_monitor_typed_stream::<u16>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::U24 => {
            build_monitor_typed_stream::<U24>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::U32 => {
            build_monitor_typed_stream::<u32>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::U64 => {
            build_monitor_typed_stream::<u64>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::F32 => {
            build_monitor_typed_stream::<f32>(device, stream_config, rms_atomic, peak_atomic)
        }
        SampleFormat::F64 => {
            build_monitor_typed_stream::<f64>(device, stream_config, rms_atomic, peak_atomic)
        }
        unsupported => Err(AudioMonitorError::UnsupportedFormat(
            unsupported.to_string(),
        )),
    }
}

fn build_monitor_typed_stream<T>(
    device: &Device,
    stream_config: &StreamConfig,
    rms_atomic: Arc<AtomicU32>,
    peak_atomic: Arc<AtomicU32>,
) -> Result<Stream, AudioMonitorError>
where
    T: cpal::SizedSample + Sample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            stream_config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut sum_sq: f32 = 0.0;
                let mut max_abs: f32 = 0.0;
                for &sample in data {
                    let s = f32::from_sample(sample);
                    sum_sq += s * s;
                    let abs = s.abs();
                    if abs > max_abs {
                        max_abs = abs;
                    }
                }
                let rms = if data.is_empty() {
                    0.0
                } else {
                    (sum_sq / data.len() as f32).sqrt()
                };
                rms_atomic.store(rms.to_bits(), Ordering::Relaxed);
                peak_atomic.store(max_abs.to_bits(), Ordering::Relaxed);
            },
            |error: cpal::StreamError| {
                eprintln!("Audio monitor stream error: {error}");
            },
            None,
        )
        .map_err(|e| AudioMonitorError::BuildStream(e.to_string()))
}
