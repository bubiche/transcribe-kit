pub mod api_openai_compatible;
pub mod local_parakeet;
pub mod local_whisper;

use crate::models::TranscriptResult;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error("Failed to load model: {0}")]
    ModelLoad(String),
    #[error("Transcription failed: {0}")]
    Inference(String),
    #[error("Audio decoding error: {0}")]
    AudioDecode(String),
    #[error("Model download failed: {0}")]
    Download(String),
    #[error("Audio encoding error: {0}")]
    AudioEncode(String),
    #[error("API transcription request failed: {0}")]
    ApiRequest(String),
}

pub async fn transcribe_api_audio_file(
    file_path: &Path,
    model_id: &str,
    credentials: &api_openai_compatible::ApiCredentials,
) -> Result<TranscriptResult, TranscriptionError> {
    api_openai_compatible::transcribe_audio_file(file_path, model_id, credentials).await
}
