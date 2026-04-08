use std::{
    env, fs,
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

mod wav_mixing;

use crate::{
    models::{LiveRecordingResult, LiveRecordingState, LiveRecordingStatus},
    recording_tray,
};

use wav_mixing::{
    duration_ms_from_frames, duration_ms_from_wav_file, mix_wav_files, wav_is_silent, wav_spec,
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
    #[error(
        "Transcribe Kit could not find a system audio output to capture the meeting mix. Connect or enable an output device, then try again."
    )]
    NoOutputDevice,
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
    #[error("Transcribe Kit could not read the temporary WAV recording data: {0}")]
    ReadWav(String),
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

enum ActiveRecording {
    Single(SingleStreamRecording),
    Dual(DualStreamRecording),
}

struct SingleStreamRecording {
    stream: Stream,
    writer_tx: Sender<Vec<i16>>,
    writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
    runtime_error: Arc<Mutex<Option<String>>>,
    startup_message: Option<String>,
    captured_frames: Arc<AtomicU64>,
    input_device_id: Option<String>,
    input_device_label: String,
    sample_rate_hz: u32,
    channels: u16,
    output_file_path: PathBuf,
}

struct DualStreamRecording {
    mic_stream: Stream,
    mic_writer_tx: Sender<Vec<i16>>,
    mic_writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
    mic_runtime_error: Arc<Mutex<Option<String>>>,
    mic_captured_frames: Arc<AtomicU64>,
    mic_sample_rate_hz: u32,
    mic_channels: u16,
    mic_temp_path: PathBuf,

    loopback_stream: Stream,
    loopback_writer_tx: Sender<Vec<i16>>,
    loopback_writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
    loopback_runtime_error: Arc<Mutex<Option<String>>>,
    loopback_captured_frames: Arc<AtomicU64>,
    loopback_temp_path: PathBuf,

    combined_output_path: PathBuf,
    input_device_id: Option<String>,
    input_device_label: String,
}

pub(crate) struct ResolvedInputDevice {
    pub(crate) device: Device,
    pub(crate) input_device_id: Option<String>,
    pub(crate) input_device_label: String,
}

impl LiveRecordingManagerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_status(&self) -> LiveRecordingStatus {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
        use_dual_capture: bool,
    ) -> Result<LiveRecordingStatus, LiveRecordingError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        if guard.active.is_some() {
            return Err(LiveRecordingError::AlreadyRecording);
        }

        let active = if use_dual_capture {
            match DualStreamRecording::start(selected_input_device_id) {
                Ok(recording) => ActiveRecording::Dual(recording),
                Err(error) => {
                    eprintln!(
                        "Dual capture unavailable ({error}). Falling back to single-stream recording."
                    );
                    let fallback_message = Some(dual_capture_fallback_message(&error));
                    ActiveRecording::Single(SingleStreamRecording::start(
                        selected_input_device_id,
                        is_output_loopback,
                        fallback_message,
                    )?)
                }
            }
        } else {
            ActiveRecording::Single(SingleStreamRecording::start(
                selected_input_device_id,
                is_output_loopback,
                None,
            )?)
        };

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
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
    fn stop(self) -> Result<LiveRecordingResult, LiveRecordingError> {
        match self {
            Self::Single(recording) => recording.stop(),
            Self::Dual(recording) => recording.stop(),
        }
    }

    fn status(&self) -> LiveRecordingStatus {
        match self {
            Self::Single(recording) => recording.status(),
            Self::Dual(recording) => recording.status(),
        }
    }
}

impl SingleStreamRecording {
    fn start(
        selected_input_device_id: Option<&str>,
        is_output_loopback: bool,
        startup_message: Option<String>,
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
            startup_message,
            captured_frames,
            input_device_id: resolved_device.input_device_id,
            input_device_label: resolved_device.input_device_label,
            sample_rate_hz,
            channels,
            output_file_path,
        })
    }

    fn stop(self) -> Result<LiveRecordingResult, LiveRecordingError> {
        let SingleStreamRecording {
            stream,
            writer_tx,
            writer_thread,
            runtime_error,
            startup_message: _,
            captured_frames,
            input_device_id,
            input_device_label,
            sample_rate_hz,
            channels,
            output_file_path,
        } = self;
        let captured_frames = captured_frames.load(Ordering::Relaxed);

        drop(stream);
        drop(writer_tx);

        match writer_thread.join() {
            Ok(result) => result?,
            Err(_) => return Err(LiveRecordingError::WriterThreadPanicked),
        }

        let runtime_error = runtime_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(runtime_error) = runtime_error {
            let _ = fs::remove_file(&output_file_path);
            return Err(LiveRecordingError::StreamRuntime(runtime_error));
        }

        Ok(LiveRecordingResult {
            file_path: output_file_path.to_string_lossy().into_owned(),
            input_device_id,
            input_device_label,
            sample_rate_hz,
            channels,
            duration_ms: duration_ms_from_frames(captured_frames, sample_rate_hz),
            is_dual_capture: false,
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
            message: self
                .runtime_error
                .lock()
                .unwrap()
                .clone()
                .or_else(|| self.startup_message.clone()),
        }
    }
}

impl DualStreamRecording {
    fn start(selected_input_device_id: Option<&str>) -> Result<Self, LiveRecordingError> {
        let resolved_mic_device = resolve_input_device(selected_input_device_id)?;
        let mic_supported_config =
            resolved_mic_device
                .device
                .default_input_config()
                .map_err(|error| {
                    LiveRecordingError::DefaultConfig(with_platform_hint(error.to_string()))
                })?;
        if mic_supported_config.sample_format().is_dsd() {
            return Err(LiveRecordingError::UnsupportedSampleFormat(
                mic_supported_config.sample_format().to_string(),
            ));
        }

        let loopback_device = cpal::default_host()
            .default_output_device()
            .ok_or(LiveRecordingError::NoOutputDevice)?;
        let loopback_supported_config =
            loopback_device.default_output_config().map_err(|error| {
                LiveRecordingError::DefaultConfig(with_platform_hint(error.to_string()))
            })?;
        if loopback_supported_config.sample_format().is_dsd() {
            return Err(LiveRecordingError::UnsupportedSampleFormat(
                loopback_supported_config.sample_format().to_string(),
            ));
        }

        let mic_stream_config = mic_supported_config.config();
        let loopback_stream_config = loopback_supported_config.config();
        let mic_sample_rate_hz = mic_supported_config.sample_rate();
        let mic_channels = mic_supported_config.channels();

        let mic_temp_path = next_recording_path();
        let (mic_writer_tx, mic_writer_thread) =
            spawn_wav_writer(&mic_temp_path, mic_sample_rate_hz, mic_channels)?;
        let loopback_temp_path = next_recording_path();
        let (loopback_writer_tx, loopback_writer_thread) = match spawn_wav_writer(
            &loopback_temp_path,
            loopback_supported_config.sample_rate(),
            loopback_supported_config.channels(),
        ) {
            Ok(result) => result,
            Err(error) => {
                cleanup_failed_start(&mic_temp_path, mic_writer_tx, mic_writer_thread);
                return Err(error);
            }
        };
        let combined_output_path = next_recording_path();

        let mic_runtime_error = Arc::new(Mutex::new(None));
        let mic_captured_frames = Arc::new(AtomicU64::new(0));
        let loopback_runtime_error = Arc::new(Mutex::new(None));
        let loopback_captured_frames = Arc::new(AtomicU64::new(0));

        let mic_stream = match build_input_stream(
            &resolved_mic_device.device,
            &mic_stream_config,
            mic_supported_config.sample_format(),
            mic_writer_tx.clone(),
            Arc::clone(&mic_captured_frames),
            Arc::clone(&mic_runtime_error),
        ) {
            Ok(stream) => stream,
            Err(error) => {
                cleanup_failed_start(&mic_temp_path, mic_writer_tx, mic_writer_thread);
                cleanup_failed_start(
                    &loopback_temp_path,
                    loopback_writer_tx,
                    loopback_writer_thread,
                );
                return Err(error);
            }
        };

        let loopback_stream = match build_input_stream(
            &loopback_device,
            &loopback_stream_config,
            loopback_supported_config.sample_format(),
            loopback_writer_tx.clone(),
            Arc::clone(&loopback_captured_frames),
            Arc::clone(&loopback_runtime_error),
        ) {
            Ok(stream) => stream,
            Err(error) => {
                drop(mic_stream);
                cleanup_failed_start(&mic_temp_path, mic_writer_tx, mic_writer_thread);
                cleanup_failed_start(
                    &loopback_temp_path,
                    loopback_writer_tx,
                    loopback_writer_thread,
                );
                return Err(error);
            }
        };

        if let Err(error) = mic_stream.play() {
            drop(loopback_stream);
            drop(mic_stream);
            cleanup_failed_start(&mic_temp_path, mic_writer_tx, mic_writer_thread);
            cleanup_failed_start(
                &loopback_temp_path,
                loopback_writer_tx,
                loopback_writer_thread,
            );
            return Err(LiveRecordingError::PlayStream(with_platform_hint(
                error.to_string(),
            )));
        }

        if let Err(error) = loopback_stream.play() {
            drop(loopback_stream);
            drop(mic_stream);
            cleanup_failed_start(&mic_temp_path, mic_writer_tx, mic_writer_thread);
            cleanup_failed_start(
                &loopback_temp_path,
                loopback_writer_tx,
                loopback_writer_thread,
            );
            return Err(LiveRecordingError::PlayStream(with_platform_hint(
                error.to_string(),
            )));
        }

        Ok(Self {
            mic_stream,
            mic_writer_tx,
            mic_writer_thread,
            mic_runtime_error,
            mic_captured_frames,
            mic_sample_rate_hz,
            mic_channels,
            mic_temp_path,
            loopback_stream,
            loopback_writer_tx,
            loopback_writer_thread,
            loopback_runtime_error,
            loopback_captured_frames,
            loopback_temp_path,
            combined_output_path,
            input_device_id: resolved_mic_device.input_device_id,
            input_device_label: resolved_mic_device.input_device_label,
        })
    }

    fn stop(self) -> Result<LiveRecordingResult, LiveRecordingError> {
        let DualStreamRecording {
            mic_stream,
            mic_writer_tx,
            mic_writer_thread,
            mic_runtime_error,
            mic_captured_frames,
            mic_sample_rate_hz,
            mic_channels,
            mic_temp_path,
            loopback_stream,
            loopback_writer_tx,
            loopback_writer_thread,
            loopback_runtime_error,
            loopback_captured_frames,
            loopback_temp_path,
            combined_output_path,
            input_device_id,
            input_device_label,
        } = self;
        let mic_captured_frames = mic_captured_frames.load(Ordering::Relaxed);
        let loopback_captured_frames = loopback_captured_frames.load(Ordering::Relaxed);

        drop(mic_stream);
        drop(loopback_stream);
        drop(mic_writer_tx);
        drop(loopback_writer_tx);

        let mic_writer_result = join_writer_thread(mic_writer_thread);
        let loopback_writer_result = join_writer_thread(loopback_writer_thread);
        let mic_runtime_error = mic_runtime_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let loopback_runtime_error = loopback_runtime_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        if let Err(error) = mic_writer_result {
            let _ = fs::remove_file(&mic_temp_path);
            let _ = fs::remove_file(&loopback_temp_path);
            let _ = fs::remove_file(&combined_output_path);
            return Err(error);
        }

        if let Some(runtime_error) = mic_runtime_error {
            let _ = fs::remove_file(&mic_temp_path);
            let _ = fs::remove_file(&loopback_temp_path);
            let _ = fs::remove_file(&combined_output_path);
            return Err(LiveRecordingError::StreamRuntime(runtime_error));
        }

        let loopback_writer_error = loopback_writer_result.err();
        if loopback_writer_error.is_some() || loopback_runtime_error.is_some() {
            if let Some(error) = loopback_writer_error {
                eprintln!("Loopback writer failed: {error}");
            }
            if let Some(runtime_error) = loopback_runtime_error {
                eprintln!("Loopback stream runtime error: {runtime_error}");
            }
            let _ = fs::remove_file(&loopback_temp_path);
            let _ = fs::remove_file(&combined_output_path);
            return Ok(single_file_result(
                &mic_temp_path,
                input_device_id,
                input_device_label,
                mic_sample_rate_hz,
                mic_channels,
                duration_ms_from_frames(mic_captured_frames, mic_sample_rate_hz),
                false,
            ));
        }

        if wav_is_silent(&loopback_temp_path) {
            eprintln!(
                "Loopback WAV is silent — system audio permission may not be granted. \
                 Falling back to microphone-only recording."
            );
            let _ = fs::remove_file(&loopback_temp_path);
            let _ = fs::remove_file(&combined_output_path);
            return Ok(single_file_result(
                &mic_temp_path,
                input_device_id,
                input_device_label,
                mic_sample_rate_hz,
                mic_channels,
                duration_ms_from_frames(mic_captured_frames, mic_sample_rate_hz),
                false,
            ));
        }

        if let Err(error) =
            mix_wav_files(&mic_temp_path, &loopback_temp_path, &combined_output_path)
        {
            eprintln!(
                "Dual capture mix failed ({error}). Falling back to microphone-only recording."
            );
            let _ = fs::remove_file(&loopback_temp_path);
            let _ = fs::remove_file(&combined_output_path);
            return Ok(single_file_result(
                &mic_temp_path,
                input_device_id,
                input_device_label,
                mic_sample_rate_hz,
                mic_channels,
                duration_ms_from_frames(mic_captured_frames, mic_sample_rate_hz),
                false,
            ));
        }

        let _ = fs::remove_file(&mic_temp_path);
        let _ = fs::remove_file(&loopback_temp_path);

        Ok(LiveRecordingResult {
            file_path: combined_output_path.to_string_lossy().into_owned(),
            input_device_id,
            input_device_label,
            sample_rate_hz: mic_sample_rate_hz,
            channels: 1,
            duration_ms: duration_ms_from_wav_file(&combined_output_path).unwrap_or_else(|_| {
                duration_ms_from_frames(
                    mic_captured_frames.max(loopback_captured_frames),
                    mic_sample_rate_hz,
                )
            }),
            is_dual_capture: true,
        })
    }

    fn status(&self) -> LiveRecordingStatus {
        let message = self
            .mic_runtime_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .or_else(|| {
                self.loopback_runtime_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                    .map(|runtime_error| format!("System audio capture: {runtime_error}"))
            });

        LiveRecordingStatus {
            state: LiveRecordingState::Recording,
            input_device_id: self.input_device_id.clone(),
            input_device_label: Some(self.input_device_label.clone()),
            output_file_path: Some(self.combined_output_path.to_string_lossy().into_owned()),
            sample_rate_hz: Some(self.mic_sample_rate_hz),
            channels: Some(self.mic_channels),
            duration_ms: Some(duration_ms_from_frames(
                self.mic_captured_frames.load(Ordering::Relaxed),
                self.mic_sample_rate_hz,
            )),
            message,
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

pub(crate) fn resolve_input_device(
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

pub(crate) fn device_label(device: &Device, fallback_id: Option<&str>) -> String {
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

fn single_file_result(
    path: &Path,
    input_device_id: Option<String>,
    input_device_label: String,
    fallback_sample_rate_hz: u32,
    fallback_channels: u16,
    fallback_duration_ms: u64,
    is_dual_capture: bool,
) -> LiveRecordingResult {
    let output = hound::WavReader::open(path)
        .ok()
        .map(|reader| {
            let spec = reader.spec();
            (
                spec.sample_rate,
                spec.channels,
                duration_ms_from_frames(reader.duration() as u64, spec.sample_rate),
            )
        })
        .unwrap_or((
            fallback_sample_rate_hz,
            fallback_channels,
            fallback_duration_ms,
        ));

    LiveRecordingResult {
        file_path: path.to_string_lossy().into_owned(),
        input_device_id,
        input_device_label,
        sample_rate_hz: output.0,
        channels: output.1,
        duration_ms: output.2,
        is_dual_capture,
    }
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

fn join_writer_thread(
    writer_thread: JoinHandle<Result<(), LiveRecordingError>>,
) -> Result<(), LiveRecordingError> {
    match writer_thread.join() {
        Ok(result) => result,
        Err(_) => Err(LiveRecordingError::WriterThreadPanicked),
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
        let mut guard = error_runtime.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut guard = runtime_error.lock().unwrap_or_else(|e| e.into_inner());
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
    let _ = fs::remove_file(path);
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

fn dual_capture_fallback_message(error: &LiveRecordingError) -> String {
    let mut message = String::from(
        "Dual capture could not start, so Transcribe Kit switched to microphone-only recording.",
    );

    if dual_capture_failure_looks_like_permission_issue(error) {
        #[cfg(target_os = "macos")]
        {
            message.push_str(" System audio recording permission appears to be blocked. Enable Transcribe Kit in System Settings > Privacy & Security > System Audio Recording, then start a new meeting capture.");
        }

        #[cfg(not(target_os = "macos"))]
        {
            message.push_str(" System audio capture permission appears to be blocked. Enable audio capture permission for Transcribe Kit in your OS privacy settings, then start a new meeting capture.");
        }
    } else if matches!(error, LiveRecordingError::NoOutputDevice) {
        message.push_str(
            " No system audio output device is currently available for loopback capture.",
        );
    }

    message
}

fn dual_capture_failure_looks_like_permission_issue(error: &LiveRecordingError) -> bool {
    let detail = match error {
        LiveRecordingError::DefaultConfig(detail)
        | LiveRecordingError::BuildStream(detail)
        | LiveRecordingError::PlayStream(detail)
        | LiveRecordingError::StreamRuntime(detail) => detail.as_str(),
        _ => return false,
    };

    let normalized = detail.to_lowercase();
    normalized.contains("permission")
        || normalized.contains("denied")
        || normalized.contains("not permitted")
        || normalized.contains("not allowed")
        || normalized.contains("access")
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
    fn next_recording_path_uses_wav_extension() {
        let path = next_recording_path();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("wav"));
        assert!(path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .contains("transcribe-kit-live-"));
    }

    #[test]
    fn dual_capture_permission_detection_matches_common_error_text() {
        assert!(dual_capture_failure_looks_like_permission_issue(
            &LiveRecordingError::PlayStream("permission denied by system".to_string()),
        ));
        assert!(dual_capture_failure_looks_like_permission_issue(
            &LiveRecordingError::BuildStream("access not permitted".to_string()),
        ));
        assert!(!dual_capture_failure_looks_like_permission_issue(
            &LiveRecordingError::NoOutputDevice,
        ));
    }

    #[test]
    fn dual_capture_fallback_message_mentions_mic_only_fallback() {
        let message = dual_capture_fallback_message(&LiveRecordingError::NoOutputDevice);
        assert!(message.contains("microphone-only recording"));
    }
}
