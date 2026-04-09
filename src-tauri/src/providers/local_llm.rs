use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use directories::ProjectDirs;
use mistralrs::{GgufModelBuilder, Model, Response, TextMessageRole, TextMessages};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use super::TranscriptionError;
use crate::models::{ModelDownloadProgress, ModelStatus};

pub const ENGINE_ID: &str = "local-llm";

pub const LLM_MODEL_IDS: &[&str] = &["llm-qwen-3-1.7b", "llm-gemma-4-e2b", "llm-gemma-4-e4b"];

struct ModelEntry {
    gguf_filename: &'static str,
    download_url: &'static str,
    size_label: &'static str,
    display_label: &'static str,
    tok_model_id: &'static str,
}

fn model_entry(model_id: &str) -> Option<&'static ModelEntry> {
    static ENTRIES: &[(&str, ModelEntry)] = &[
        (
            "llm-qwen-3-1.7b",
            ModelEntry {
                gguf_filename: "Qwen3-1.7B-Q4_K_M.gguf",
                download_url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q4_k_m.gguf",
                size_label: "~1.2 GB",
                display_label: "Qwen 3 1.7B (Default)",
                tok_model_id: "Qwen/Qwen3-1.7B",
            },
        ),
        (
            "llm-gemma-4-e2b",
            ModelEntry {
                gguf_filename: "gemma-3n-E2B-it-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bartowski/gemma-3n-E2B-it-GGUF/resolve/main/gemma-3n-E2B-it-Q4_K_M.gguf",
                size_label: "~3.5 GB",
                display_label: "Gemma 4 E2B",
                tok_model_id: "google/gemma-3n-E2B-it",
            },
        ),
        (
            "llm-gemma-4-e4b",
            ModelEntry {
                gguf_filename: "gemma-3n-E4B-it-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bartowski/gemma-3n-E4B-it-GGUF/resolve/main/gemma-3n-E4B-it-Q4_K_M.gguf",
                size_label: "~5.3 GB",
                display_label: "Gemma 4 E4B",
                tok_model_id: "google/gemma-3n-E4B-it",
            },
        ),
    ];

    ENTRIES
        .iter()
        .find(|(id, _)| *id == model_id)
        .map(|(_, e)| e)
}

pub fn is_known_model_id(model_id: &str) -> bool {
    model_entry(model_id).is_some()
}

pub fn display_label(model_id: &str) -> &'static str {
    model_entry(model_id)
        .map(|e| e.display_label)
        .unwrap_or("Unknown")
}

pub fn size_label(model_id: &str) -> &'static str {
    model_entry(model_id)
        .map(|e| e.size_label)
        .unwrap_or("Unknown")
}

// ---------------------------------------------------------------------------
// Cache management
// ---------------------------------------------------------------------------

fn cache_dir() -> Result<PathBuf, TranscriptionError> {
    let project_dirs =
        ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit").ok_or_else(|| {
            TranscriptionError::ModelLoad("Could not determine cache directory".to_string())
        })?;
    Ok(project_dirs.cache_dir().join("llm-models"))
}

pub fn expected_model_path(model_id: &str) -> Result<PathBuf, TranscriptionError> {
    let entry = model_entry(model_id).ok_or_else(|| {
        TranscriptionError::ModelLoad(format!("No GGUF model file mapped for '{model_id}'"))
    })?;
    Ok(cache_dir()?.join(entry.gguf_filename))
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

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

pub async fn download_model(
    model_id: &str,
    on_progress: &Channel<ModelDownloadProgress>,
) -> Result<PathBuf, TranscriptionError> {
    let path = expected_model_path(model_id)?;

    if path.exists() {
        return Ok(path);
    }

    let entry = model_entry(model_id).ok_or_else(|| {
        TranscriptionError::Download(format!("No download URL for model '{model_id}'"))
    })?;

    let models_dir = path
        .parent()
        .ok_or_else(|| TranscriptionError::Download("Invalid model path".to_string()))?;
    std::fs::create_dir_all(models_dir).map_err(|e| {
        TranscriptionError::Download(format!("Failed to create models directory: {e}"))
    })?;

    let temp_path = path.with_extension("gguf.download");

    let result = do_download(model_id, entry.download_url, &path, &temp_path, on_progress).await;

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    result
}

async fn do_download(
    model_id: &str,
    url: &str,
    final_path: &Path,
    temp_path: &Path,
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

    Ok(final_path.to_path_buf())
}

// ---------------------------------------------------------------------------
// LlmEngine
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct LlmEngine {
    model: Arc<Model>,
    model_id: String,
}

impl LlmEngine {
    pub async fn load(
        model_dir: &str,
        gguf_filename: &str,
        tok_model_id: &str,
        model_id: String,
    ) -> Result<Self, TranscriptionError> {
        let model = GgufModelBuilder::new(model_dir, vec![gguf_filename.to_string()])
            .with_tok_model_id(tok_model_id)
            .with_force_cpu()
            .build()
            .await
            .map_err(|e| TranscriptionError::ModelLoad(format!("{e}")))?;

        Ok(Self {
            model: Arc::new(model),
            model_id,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Run a chat completion using streaming internally so that the request
    /// can be cancelled via the provided `CancellationToken`.
    ///
    /// When the token is cancelled the stream receiver is dropped, which
    /// causes the mistral.rs engine to stop generation at the next token
    /// boundary.
    pub async fn chat_completion(
        &self,
        prompt: &str,
        cancel_token: CancellationToken,
    ) -> Result<String, TranscriptionError> {
        let messages = TextMessages::new().add_message(TextMessageRole::User, prompt);

        let mut stream = self
            .model
            .stream_chat_request(messages)
            .await
            .map_err(|e| TranscriptionError::Inference(format!("{e}")))?;

        let mut result = String::new();
        loop {
            tokio::select! {
                chunk = stream.next() => {
                    match chunk {
                        Some(response) => {
                            match response {
                                Response::Chunk(chunk_resp) => {
                                    if let Some(choice) = chunk_resp.choices.first() {
                                        if let Some(ref content) = choice.delta.content {
                                            result.push_str(content);
                                        }
                                    }
                                }
                                Response::Done(_) => break,
                                Response::ModelError(msg, _) => {
                                    return Err(TranscriptionError::Inference(
                                        format!("Model error: {msg}")
                                    ));
                                }
                                _ => {}
                            }
                        }
                        None => break,
                    }
                }
                _ = cancel_token.cancelled() => {
                    drop(stream);
                    return Err(TranscriptionError::Inference(
                        "Post-processing was cancelled.".to_string()
                    ));
                }
            }
        }

        Ok(result.trim().to_string())
    }

    /// Load an engine from a known model ID, resolving paths from the registry.
    pub async fn load_by_model_id(model_id: &str) -> Result<Self, TranscriptionError> {
        let entry = model_entry(model_id).ok_or_else(|| {
            TranscriptionError::ModelLoad(format!("Unknown LLM model ID: '{model_id}'"))
        })?;

        let model_path = resolve_model_path(model_id)?;
        let model_dir = model_path
            .parent()
            .ok_or_else(|| TranscriptionError::ModelLoad("Invalid model path".to_string()))?
            .to_string_lossy()
            .to_string();

        Self::load(
            &model_dir,
            entry.gguf_filename,
            entry.tok_model_id,
            model_id.to_string(),
        )
        .await
    }
}
