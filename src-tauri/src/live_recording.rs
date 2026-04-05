use std::{
    env,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, Sample, SampleFormat, Stream, StreamConfig, I24, U24,
};
use tauri::{AppHandle, Emitter, Runtime};

use crate::{
    models::{LiveRecordingResult, LiveRecordingState, LiveRecordingStatus},
    recording_tray,
};

pub const LIVE_RECORDING_STATUS_EVENT_NAME: &str = "transcribe-kit://live-recording-status";

#[derive(Debug, thiserror::Error)]
pub enum LiveRecordingError {
    #[error("A live recording is already in progress. Stop it before starting a new recording.")]
    AlreadyRecording,
    #[error("No live recording is in progress right now.")]
    NotRecording,
    #[error("Transcribe Kit could not find an audio input to start recording. Connect an audio input or choose a different device in Settings.{0}")]
    NoInputDevice(&'static str),
    #[error("Transcribe Kit could not find the selected audio input anymore. Re-open Settings and choose an available input device.")]
    SelectedDeviceUnavailable,
    #[error("Transcribe Kit could not reach the audio backend for the selected input device.")]
    HostUnavailable,
    #[error("Transcribe Kit could not determine a usable capture format for the selected input device: {0}")]
    DefaultConfig(String),
    #[error(
        "Transcribe Kit does not support recording from this input device sample format: \"{0}\"."
    )]
    UnsupportedSampleFormat(String),
    #[error("Transcribe Kit could not create the temporary WAV recording: {0}")]
    CreateWav(String),
    #[error("Transcribe Kit could not start the audio input stream: {0}")]
    BuildStream(String),
    #[error("Transcribe Kit could not begin recording from the audio input: {0}")]
    PlayStream(String),
    #[error("Transcribe Kit could not finish writing the temporary WAV recording: {0}")]
    FinalizeWav(String),
    #[error("The live recording worker stopped unexpectedly.")]
    WriterThreadPanicked,
    #[error("The live recording stream reported an error: {0}")]
    StreamRuntime(String),
}

#[derive(Clone, Default)]
pub struct LiveRecordingManagerState {
    inner: Arc<Mutex<LiveRecordingManager>>,
}

#[derive(Default)]
struct LiveRecordingManager {
    active: Option<ActiveRecording>,
}

struct ActiveRecording {
    stream: Stream,
    writer_tx: Sender<Vec<i16>>,
    writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
    runtime_error: Arc<Mutex<Option<String>>>,
    captured_frames: Arc<AtomicU64>,
    input_device_id: Option<String>,
    input_device_label: String,
    sample_rate_hz: u32,
    channels: u16,
    output_file_path: PathBuf,
}

struct ResolvedInputDevice {
    device: Device,
    input_device_id: Option<String>,
    input_device_label: String,
}

impl LiveRecordingManagerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_status(&self) -> LiveRecordingStatus {
        let guard = self.inner.lock().unwrap();
        guard
            .active
            .as_ref()
            .map(ActiveRecording::status)
            .unwrap_or_else(LiveRecordingStatus::idle)
    }

    pub fn start<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        selected_input_device_id: Option<&str>,
        is_output_loopback: bool,
    ) -> Result<LiveRecordingStatus, LiveRecordingError> {
        let mut guard = self.inner.lock().unwrap();

        if guard.active.is_some() {
            return Err(LiveRecordingError::AlreadyRecording);
        }

        let active = ActiveRecording::start(selected_input_device_id, is_output_loopback)?;
        let status = active.status();
        guard.active = Some(active);
        drop(guard);

        emit_status(app, &status);
        recording_tray::set_recording(app, &status);
        Ok(status)
    }

    pub fn stop<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<LiveRecordingResult, LiveRecordingError> {
        let active = {
            let mut guard = self.inner.lock().unwrap();
            guard
                .active
                .take()
                .ok_or(LiveRecordingError::NotRecording)?
        };

        let result = active.stop();
        emit_status(app, &LiveRecordingStatus::idle());
        recording_tray::set_idle(app);
        result
    }
}

impl ActiveRecording {
    fn start(
        selected_input_device_id: Option<&str>,
        is_output_loopback: bool,
    ) -> Result<Self, LiveRecordingError> {
        let resolved_device = resolve_input_device(selected_input_device_id)?;
        let supported_config = if is_output_loopback {
            resolved_device.device.default_output_config()
        } else {
            resolved_device.device.default_input_config()
        }
        .map_err(|error| {
            LiveRecordingError::DefaultConfig(with_platform_hint(error.to_string()))
        })?;

        if supported_config.sample_format().is_dsd() {
            return Err(LiveRecordingError::UnsupportedSampleFormat(
                supported_config.sample_format().to_string(),
            ));
        }

        let stream_config = supported_config.config();
        let sample_rate_hz = supported_config.sample_rate();
        let channels = supported_config.channels();
        let output_file_path = next_recording_path();
        let (writer_tx, writer_thread) =
            spawn_wav_writer(&output_file_path, sample_rate_hz, channels)?;
        let runtime_error = Arc::new(Mutex::new(None));
        let captured_frames = Arc::new(AtomicU64::new(0));

        let stream = match build_input_stream(
            &resolved_device.device,
            &stream_config,
            supported_config.sample_format(),
            writer_tx.clone(),
            Arc::clone(&captured_frames),
            Arc::clone(&runtime_error),
        ) {
            Ok(stream) => stream,
            Err(error) => {
                cleanup_failed_start(output_file_path.as_path(), writer_tx, writer_thread);
                return Err(error);
            }
        };

        if let Err(error) = stream.play() {
            cleanup_failed_start(output_file_path.as_path(), writer_tx, writer_thread);
            return Err(LiveRecordingError::PlayStream(with_platform_hint(
                error.to_string(),
            )));
        }

        Ok(Self {
            stream,
            writer_tx,
            writer_thread,
            runtime_error,
            captured_frames,
            input_device_id: resolved_device.input_device_id,
            input_device_label: resolved_device.input_device_label,
            sample_rate_hz,
            channels,
            output_file_path,
        })
    }

    fn stop(self) -> Result<LiveRecordingResult, LiveRecordingError> {
        let output_file_path = self.output_file_path.clone();
        let sample_rate_hz = self.sample_rate_hz;
        let channels = self.channels;
        let input_device_id = self.input_device_id.clone();
        let input_device_label = self.input_device_label.clone();
        let captured_frames = self.captured_frames.load(Ordering::Relaxed);
        let runtime_error = self.runtime_error.lock().unwrap().clone();

        drop(self.stream);
        drop(self.writer_tx);

        match self.writer_thread.join() {
            Ok(result) => result?,
            Err(_) => return Err(LiveRecordingError::WriterThreadPanicked),
        }

        if let Some(runtime_error) = runtime_error {
            let _ = std::fs::remove_file(&output_file_path);
            return Err(LiveRecordingError::StreamRuntime(runtime_error));
        }

        Ok(LiveRecordingResult {
            file_path: output_file_path.to_string_lossy().into_owned(),
            input_device_id,
            input_device_label,
            sample_rate_hz,
            channels,
            duration_ms: duration_ms_from_frames(captured_frames, sample_rate_hz),
        })
    }

    fn status(&self) -> LiveRecordingStatus {
        LiveRecordingStatus {
            state: LiveRecordingState::Recording,
            input_device_id: self.input_device_id.clone(),
            input_device_label: Some(self.input_device_label.clone()),
            output_file_path: Some(self.output_file_path.to_string_lossy().into_owned()),
            sample_rate_hz: Some(self.sample_rate_hz),
            channels: Some(self.channels),
            duration_ms: Some(duration_ms_from_frames(
                self.captured_frames.load(Ordering::Relaxed),
                self.sample_rate_hz,
            )),
            message: self.runtime_error.lock().unwrap().clone(),
        }
    }
}

impl LiveRecordingStatus {
    fn idle() -> Self {
        Self {
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
}

fn emit_status<R: Runtime>(app: &AppHandle<R>, status: &LiveRecordingStatus) {
    let _ = app.emit(LIVE_RECORDING_STATUS_EVENT_NAME, status);
}

fn resolve_input_device(
    selected_input_device_id: Option<&str>,
) -> Result<ResolvedInputDevice, LiveRecordingError> {
    if let Some(selected_input_device_id) = selected_input_device_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let device_id = selected_input_device_id
            .parse::<cpal::DeviceId>()
            .map_err(|_| LiveRecordingError::SelectedDeviceUnavailable)?;
        let host =
            cpal::host_from_id(device_id.0).map_err(|_| LiveRecordingError::HostUnavailable)?;
        let device = host
            .device_by_id(&device_id)
            .ok_or(LiveRecordingError::SelectedDeviceUnavailable)?;

        Ok(ResolvedInputDevice {
            input_device_id: Some(selected_input_device_id.to_string()),
            input_device_label: device_label(&device, Some(selected_input_device_id)),
            device,
        })
    } else {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(LiveRecordingError::NoInputDevice(no_input_device_hint()))?;
        let input_device_id = device.id().ok().map(|device_id| device_id.to_string());
        let input_device_label = device_label(&device, input_device_id.as_deref());

        Ok(ResolvedInputDevice {
            device,
            input_device_id,
            input_device_label,
        })
    }
}

fn device_label(device: &Device, fallback_id: Option<&str>) -> String {
    device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| {
            fallback_id
                .map(short_device_label)
                .unwrap_or_else(|| "System audio input".to_string())
        })
}

fn short_device_label(device_id: &str) -> String {
    let suffix = device_id.rsplit(':').next().unwrap_or(device_id);
    format!("Audio input {}", &suffix[..8.min(suffix.len())])
}

fn next_recording_path() -> PathBuf {
    let temp_dir = env::temp_dir();
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let process_id = std::process::id();

    for attempt in 0..32_u32 {
        let path = temp_dir.join(format!(
            "transcribe-kit-live-{timestamp_ms}-{process_id}-{attempt}.wav"
        ));
        if !path.exists() {
            return path;
        }
    }

    temp_dir.join(format!("transcribe-kit-live-{process_id}.wav"))
}

fn spawn_wav_writer(
    path: &Path,
    sample_rate_hz: u32,
    channels: u16,
) -> Result<(Sender<Vec<i16>>, JoinHandle<Result<(), LiveRecordingError>>), LiveRecordingError> {
    let writer = hound::WavWriter::create(path, wav_spec(sample_rate_hz, channels))
        .map_err(|error| LiveRecordingError::CreateWav(error.to_string()))?;
    let (tx, rx) = mpsc::channel::<Vec<i16>>();

    let writer_thread = thread::spawn(move || -> Result<(), LiveRecordingError> {
        let mut writer = writer;

        while let Ok(chunk) = rx.recv() {
            for sample in chunk {
                writer
                    .write_sample(sample)
                    .map_err(|error| LiveRecordingError::FinalizeWav(error.to_string()))?;
            }
        }

        writer
            .finalize()
            .map_err(|error| LiveRecordingError::FinalizeWav(error.to_string()))?;
        Ok(())
    });

    Ok((tx, writer_thread))
}

fn wav_spec(sample_rate_hz: u32, channels: u16) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

fn build_input_stream(
    device: &Device,
    stream_config: &StreamConfig,
    sample_format: SampleFormat,
    writer_tx: Sender<Vec<i16>>,
    captured_frames: Arc<AtomicU64>,
    runtime_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, LiveRecordingError> {
    let channels = stream_config.channels as usize;

    match sample_format {
        SampleFormat::I8 => build_typed_input_stream::<i8>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::I16 => build_typed_input_stream::<i16>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::I24 => build_typed_input_stream::<I24>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::I32 => build_typed_input_stream::<i32>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::I64 => build_typed_input_stream::<i64>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::U8 => build_typed_input_stream::<u8>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::U16 => build_typed_input_stream::<u16>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::U24 => build_typed_input_stream::<U24>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::U32 => build_typed_input_stream::<u32>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::U64 => build_typed_input_stream::<u64>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::F32 => build_typed_input_stream::<f32>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        SampleFormat::F64 => build_typed_input_stream::<f64>(
            device,
            stream_config,
            writer_tx,
            captured_frames,
            runtime_error,
            channels,
        ),
        unsupported => Err(LiveRecordingError::UnsupportedSampleFormat(
            unsupported.to_string(),
        )),
    }
}

fn build_typed_input_stream<T>(
    device: &Device,
    stream_config: &StreamConfig,
    writer_tx: Sender<Vec<i16>>,
    captured_frames: Arc<AtomicU64>,
    runtime_error: Arc<Mutex<Option<String>>>,
    channels: usize,
) -> Result<Stream, LiveRecordingError>
where
    T: cpal::SizedSample,
    T: Sample,
    i16: FromSample<T>,
{
    let error_runtime = Arc::clone(&runtime_error);
    let error_callback = move |error: cpal::StreamError| {
        let mut guard = error_runtime.lock().unwrap();
        if guard.is_none() {
            *guard = Some(with_platform_hint(error.to_string()));
        }
    };

    device
        .build_input_stream(
            stream_config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                capture_chunk(data, channels, &writer_tx, &captured_frames, &runtime_error);
            },
            error_callback,
            None,
        )
        .map_err(|error| LiveRecordingError::BuildStream(with_platform_hint(error.to_string())))
}

fn capture_chunk<T>(
    input: &[T],
    channels: usize,
    writer_tx: &Sender<Vec<i16>>,
    captured_frames: &AtomicU64,
    runtime_error: &Mutex<Option<String>>,
) where
    T: Sample,
    i16: FromSample<T>,
{
    let frames = if channels == 0 {
        0
    } else {
        input.len() / channels
    };
    captured_frames.fetch_add(frames as u64, Ordering::Relaxed);

    let chunk = input
        .iter()
        .copied()
        .map(i16::from_sample)
        .collect::<Vec<_>>();
    if let Err(error) = writer_tx.send(chunk) {
        let mut guard = runtime_error.lock().unwrap();
        if guard.is_none() {
            *guard = Some(format!(
                "Transcribe Kit stopped receiving audio input data: {error}"
            ));
        }
    }
}

fn cleanup_failed_start(
    path: &Path,
    writer_tx: Sender<Vec<i16>>,
    writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
) {
    drop(writer_tx);
    let _ = writer_thread.join();
    let _ = std::fs::remove_file(path);
}

fn duration_ms_from_frames(frame_count: u64, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }

    (frame_count.saturating_mul(1000)) / sample_rate_hz as u64
}

fn no_input_device_hint() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        " On Linux, make sure PipeWire, PulseAudio, or ALSA input is available."
    }

    #[cfg(target_os = "macos")]
    {
        " On macOS, also confirm microphone access is allowed in System Settings > Privacy & Security > Microphone."
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        ""
    }
}

fn with_platform_hint(message: String) -> String {
    #[cfg(target_os = "linux")]
    {
        format!("{message} If this is Linux, also check whether PipeWire or PulseAudio is running and whether another app has locked the audio input.")
    }

    #[cfg(target_os = "macos")]
    {
        format!("{message} If this is macOS, verify microphone access in System Settings > Privacy & Security > Microphone.")
    }

    #[cfg(target_os = "windows")]
    {
        format!("{message} If this is Windows, check whether another app already has the audio input open.")
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn writer_thread_persists_i16_samples_to_wav() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("capture.wav");
        let (tx, join) = spawn_wav_writer(&path, 16_000, 1).expect("spawn writer");

        tx.send(vec![100, -100, 25]).expect("send samples");
        drop(tx);

        join.join()
            .expect("thread should not panic")
            .expect("writer should finalize");

        let mut reader = hound::WavReader::open(&path).expect("open wav");
        let spec = reader.spec();

        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);

        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .expect("samples");
        assert_eq!(samples, vec![100, -100, 25]);
    }

    #[test]
    fn duration_ms_uses_frame_count() {
        assert_eq!(duration_ms_from_frames(16_000, 16_000), 1000);
        assert_eq!(duration_ms_from_frames(8_000, 16_000), 500);
    }

    #[test]
    fn next_recording_path_uses_wav_extension() {
        let path = next_recording_path();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("wav"));
        assert!(path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .contains("transcribe-kit-live-"));
    }
}
