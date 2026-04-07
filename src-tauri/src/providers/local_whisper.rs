use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use directories::ProjectDirs;
use tauri::ipc::Channel;
use whisper_rs::{
    FullParams, SamplingStrategy, SegmentCallbackData, WhisperContext, WhisperContextParameters,
};

use super::TranscriptionError;
use crate::models::{
    InputType, ModelDownloadProgress, ModelStatus, TranscriptResult, TranscriptSegment,
    TranscriptionSource,
};

pub const ENGINE_ID: &str = "whisper";

#[derive(Clone)]
pub struct WhisperEngine {
    context: Arc<WhisperContext>,
    model_id: String,
}

impl WhisperEngine {
    pub fn load(model_path: &str, model_id: String) -> Result<Self, TranscriptionError> {
        let context =
            WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
                .map_err(|e| TranscriptionError::ModelLoad(format!("{e}")))?;

        Ok(Self {
            context: Arc::new(context),
            model_id,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl WhisperEngine {
    pub fn transcribe_pcm_streaming<FP, FS>(
        &self,
        samples: &[f32],
        on_progress: Option<FP>,
        on_segment: Option<FS>,
    ) -> Result<TranscriptResult, TranscriptionError>
    where
        FP: FnMut(i32) + 'static,
        FS: FnMut(i32, TranscriptSegment, String) + 'static,
    {
        let mut state = self
            .context
            .create_state()
            .map_err(|e| TranscriptionError::Inference(format!("{e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4)
            .min(8);

        params.set_n_threads(n_threads);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        if let Some(on_progress) = on_progress {
            let callback: Box<dyn FnMut(i32)> = Box::new(on_progress);
            params.set_progress_callback_safe::<Option<Box<dyn FnMut(i32)>>, Box<dyn FnMut(i32)>>(
                Some(callback),
            );
        }

        if let Some(mut on_segment) = on_segment {
            let mut accumulated_text = String::new();
            let callback: Box<dyn FnMut(SegmentCallbackData)> =
                Box::new(move |segment_data: SegmentCallbackData| {
                    let segment = callback_segment_to_model(&segment_data);
                    accumulated_text.push_str(&segment.text);
                    on_segment(
                        segment_data.segment,
                        segment,
                        accumulated_text.trim().to_string(),
                    );
                });
            params.set_segment_callback_safe_lossy::<
                Option<Box<dyn FnMut(SegmentCallbackData)>>,
                Box<dyn FnMut(SegmentCallbackData)>,
            >(Some(callback));
        }

        state
            .full(params, samples)
            .map_err(|e| TranscriptionError::Inference(format!("{e}")))?;

        let mut segments = Vec::new();
        let mut full_text = String::new();

        for segment in state.as_iter() {
            let text = segment
                .to_str()
                .map_err(|e| TranscriptionError::Inference(format!("{e}")))?
                .to_string();
            let start = segment.start_timestamp();
            let end = segment.end_timestamp();

            segments.push(TranscriptSegment {
                start_ms: start * 10,
                end_ms: end * 10,
                text: text.clone(),
            });
            full_text.push_str(&text);
        }

        Ok(TranscriptResult {
            text: full_text.trim().to_string(),
            segments,
            source: TranscriptionSource {
                provider: ENGINE_ID.to_string(),
                model_id: self.model_id.clone(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: None,
                duration_ms: None,
            },
            post_processed_text: None,
        })
    }
}

fn callback_segment_to_model(segment_data: &whisper_rs::SegmentCallbackData) -> TranscriptSegment {
    TranscriptSegment {
        start_ms: segment_data.start_timestamp * 10,
        end_ms: segment_data.end_timestamp * 10,
        text: segment_data.text.clone(),
    }
}

pub fn resolve_model_path(model_id: &str) -> Result<PathBuf, TranscriptionError> {
    let path = expected_model_path(model_id)?;

    if !path.exists() {
        return Err(TranscriptionError::ModelLoad(format!(
            "Model '{}' is not downloaded. Please download it first.",
            model_id
        )));
    }

    Ok(path)
}

pub fn expected_model_path(model_id: &str) -> Result<PathBuf, TranscriptionError> {
    let filename = ggml_filename(model_id).ok_or_else(|| {
        TranscriptionError::ModelLoad(format!("No GGML model file mapped for '{model_id}'"))
    })?;

    let project_dirs =
        ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit").ok_or_else(|| {
            TranscriptionError::ModelLoad("Could not determine cache directory".to_string())
        })?;

    Ok(project_dirs.cache_dir().join("models").join(filename))
}

pub fn ggml_filename(model_id: &str) -> Option<&'static str> {
    match model_id {
        "whisper-tiny" => Some("ggml-tiny.bin"),
        "whisper-base" => Some("ggml-base.bin"),
        "whisper-small" => Some("ggml-small.bin"),
        "whisper-large-v3-turbo" => Some("ggml-large-v3-turbo.bin"),
        _ => None,
    }
}

fn download_url(model_id: &str) -> Option<&'static str> {
    match model_id {
        "whisper-tiny" => {
            Some("https://huggingface.co/bubiche/whisper.cpp/resolve/main/ggml-tiny.bin")
        }
        "whisper-base" => {
            Some("https://huggingface.co/bubiche/whisper.cpp/resolve/main/ggml-base.bin")
        }
        "whisper-small" => {
            Some("https://huggingface.co/bubiche/whisper.cpp/resolve/main/ggml-small.bin")
        }
        "whisper-large-v3-turbo" => {
            Some("https://huggingface.co/bubiche/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin")
        }
        _ => None,
    }
}

pub fn size_label(model_id: &str) -> &'static str {
    match model_id {
        "whisper-tiny" => "~75 MB",
        "whisper-base" => "~148 MB",
        "whisper-small" => "~488 MB",
        "whisper-large-v3-turbo" => "~809 MB",
        _ => "Unknown",
    }
}

pub fn model_status(model_id: &str) -> Result<ModelStatus, TranscriptionError> {
    let path = expected_model_path(model_id)?;
    let downloaded = path.exists();
    let size_bytes = if downloaded {
        std::fs::metadata(&path).ok().map(|m| m.len())
    } else {
        None
    };

    Ok(ModelStatus {
        model_id: model_id.to_string(),
        downloaded,
        size_bytes,
    })
}

pub fn delete_model(model_id: &str) -> Result<(), TranscriptionError> {
    let path = expected_model_path(model_id)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| {
            TranscriptionError::ModelLoad(format!("Failed to delete model file: {e}"))
        })?;
    }
    Ok(())
}

pub async fn download_model(
    model_id: &str,
    on_progress: &Channel<ModelDownloadProgress>,
) -> Result<PathBuf, TranscriptionError> {
    let path = expected_model_path(model_id)?;

    if path.exists() {
        return Ok(path);
    }

    let url = download_url(model_id).ok_or_else(|| {
        TranscriptionError::Download(format!("No download URL for model '{model_id}'"))
    })?;

    let models_dir = path
        .parent()
        .ok_or_else(|| TranscriptionError::Download("Invalid model path".to_string()))?;
    std::fs::create_dir_all(models_dir).map_err(|e| {
        TranscriptionError::Download(format!("Failed to create models directory: {e}"))
    })?;

    let temp_path = path.with_extension("bin.download");

    let result = do_download(model_id, url, &path, &temp_path, on_progress).await;

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    result
}

async fn do_download(
    model_id: &str,
    url: &str,
    final_path: &PathBuf,
    temp_path: &PathBuf,
    on_progress: &Channel<ModelDownloadProgress>,
) -> Result<PathBuf, TranscriptionError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| TranscriptionError::Download(format!("HTTP request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(TranscriptionError::Download(format!(
            "Download returned HTTP {}",
            response.status()
        )));
    }

    let total_bytes = response.content_length();
    let mut file = std::fs::File::create(temp_path)
        .map_err(|e| TranscriptionError::Download(format!("Failed to create temp file: {e}")))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| TranscriptionError::Download(format!("Download interrupted: {e}")))?
    {
        file.write_all(&chunk)
            .map_err(|e| TranscriptionError::Download(format!("Failed to write chunk: {e}")))?;

        downloaded += chunk.len() as u64;

        if downloaded - last_emit > 256 * 1024 || downloaded == total_bytes.unwrap_or(0) {
            last_emit = downloaded;
            let _ = on_progress.send(ModelDownloadProgress {
                model_id: model_id.to_string(),
                downloaded_bytes: downloaded,
                total_bytes,
                done: false,
            });
        }
    }

    file.flush()
        .map_err(|e| TranscriptionError::Download(format!("Failed to flush file: {e}")))?;
    drop(file);

    std::fs::rename(temp_path, final_path)
        .map_err(|e| TranscriptionError::Download(format!("Failed to finalize download: {e}")))?;

    let _ = on_progress.send(ModelDownloadProgress {
        model_id: model_id.to_string(),
        downloaded_bytes: downloaded,
        total_bytes: Some(downloaded),
        done: true,
    });

    Ok(final_path.clone())
}
