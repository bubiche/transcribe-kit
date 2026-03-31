pub mod api_openai_compatible;
pub mod local_parakeet;
pub mod local_whisper;

use crate::models::TranscriptResult;

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
}

pub trait TranscribeLocal: Send + Sync {
    fn transcribe_pcm(&self, samples: &[f32]) -> Result<TranscriptResult, TranscriptionError>;
}
