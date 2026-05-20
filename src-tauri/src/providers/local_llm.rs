use std::io::Write;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use tauri::ipc::Channel;

use super::TranscriptionError;
use crate::models::{ModelDownloadProgress, ModelStatus};

pub const ENGINE_ID: &str = "local-llm";

pub const LLM_MODEL_IDS: &[&str] = &[
    "llm-qwen-3.5-0.8b",
    "llm-qwen-3.5-4b",
    "llm-gemma-4-e2b",
    "llm-gemma-4-e4b",
];

struct ModelEntry {
    gguf_filename: &'static str,
    download_url: &'static str,
    size_label: &'static str,
    display_label: &'static str,
}

fn model_entry(model_id: &str) -> Option<&'static ModelEntry> {
    static ENTRIES: &[(&str, ModelEntry)] = &[
        (
            "llm-qwen-3.5-0.8b",
            ModelEntry {
                gguf_filename: "Qwen3.5-0.8B-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bubiche/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf",
                size_label: "~0.50 GB",
                display_label: "Qwen 3.5 0.8B (Default)",
            },
        ),
        (
            "llm-qwen-3.5-4b",
            ModelEntry {
                gguf_filename: "Qwen3.5-4B-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bubiche/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf",
                size_label: "~2.55 GB",
                display_label: "Qwen 3.5 4B",
            },
        ),
        (
            "llm-gemma-4-e2b",
            ModelEntry {
                gguf_filename: "gemma-4-E2B-it-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bubiche/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-Q4_K_M.gguf",
                size_label: "~2.89 GB",
                display_label: "Gemma 4 E2B",
            },
        ),
        (
            "llm-gemma-4-e4b",
            ModelEntry {
                gguf_filename: "gemma-4-E4B-it-Q4_K_M.gguf",
                download_url: "https://huggingface.co/bubiche/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf",
                size_label: "~4.64 GB",
                display_label: "Gemma 4 E4B",
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
    Ok(status_for_path(model_id, &path))
}

fn status_for_path(model_id: &str, path: &Path) -> ModelStatus {
    let downloaded = path.exists();
    let size_bytes = if downloaded {
        std::fs::metadata(path).ok().map(|m| m.len())
    } else {
        None
    };

    ModelStatus {
        model_id: model_id.to_string(),
        downloaded,
        size_bytes,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_model_ids_are_recognized() {
        for id in LLM_MODEL_IDS {
            assert!(is_known_model_id(id), "expected '{id}' to be recognized");
        }
    }

    #[test]
    fn unknown_model_id_is_rejected() {
        assert!(!is_known_model_id("not-a-model"));
        assert!(!is_known_model_id(""));
    }

    #[test]
    fn display_labels_are_non_empty() {
        for id in LLM_MODEL_IDS {
            let label = display_label(id);
            assert_ne!(label, "Unknown", "missing display label for '{id}'");
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn unknown_model_display_label_is_unknown() {
        assert_eq!(display_label("bogus"), "Unknown");
    }

    #[test]
    fn size_labels_are_non_empty() {
        for id in LLM_MODEL_IDS {
            let label = size_label(id);
            assert_ne!(label, "Unknown", "missing size label for '{id}'");
            assert!(label.starts_with('~'), "size label should start with '~'");
        }
    }

    #[test]
    fn expected_model_path_contains_llm_models_dir() {
        for id in LLM_MODEL_IDS {
            let path = expected_model_path(id).expect("valid path");
            assert!(
                path.to_string_lossy().contains("llm-models"),
                "path should use llm-models dir, got: {path:?}"
            );
            assert!(
                path.extension().is_some_and(|ext| ext == "gguf"),
                "path should end in .gguf, got: {path:?}"
            );
        }
    }

    #[test]
    fn expected_model_path_rejects_unknown_id() {
        let result = expected_model_path("not-a-model");
        assert!(result.is_err());
    }

    #[test]
    fn status_for_path_reports_not_downloaded_when_file_missing() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        for id in LLM_MODEL_IDS {
            let path = temp.path().join(format!("{id}.gguf"));
            let status = status_for_path(id, &path);
            assert_eq!(status.model_id, *id);
            assert!(!status.downloaded);
            assert!(status.size_bytes.is_none());
        }
    }

    #[test]
    fn status_for_path_reports_downloaded_with_size_when_file_present() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("model.gguf");
        std::fs::write(&path, b"hello world").expect("write model");
        let status = status_for_path("llm-qwen-3.5-0.8b", &path);
        assert_eq!(status.model_id, "llm-qwen-3.5-0.8b");
        assert!(status.downloaded);
        assert_eq!(status.size_bytes, Some(11));
    }

    #[test]
    fn registry_entries_all_have_gguf_filenames() {
        for id in LLM_MODEL_IDS {
            let entry = model_entry(id).expect("registry entry");
            assert!(
                entry.gguf_filename.ends_with(".gguf"),
                "filename should end with .gguf: {}",
                entry.gguf_filename
            );
        }
    }

    #[test]
    fn registry_entries_all_have_download_urls() {
        for id in LLM_MODEL_IDS {
            let entry = model_entry(id).expect("registry entry");
            assert!(
                entry.download_url.starts_with("https://"),
                "download URL should be https: {}",
                entry.download_url
            );
        }
    }

    #[test]
    fn llm_model_ids_array_matches_registry() {
        for id in LLM_MODEL_IDS {
            assert!(
                model_entry(id).is_some(),
                "LLM_MODEL_IDS contains '{id}' but registry has no entry"
            );
        }
    }
}
