use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderMode {
    Local,
    Api,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key_present: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            provider_mode: ProviderMode::Local,
            local_model_id: "whisper-base".to_string(),
            api_model_id: "gpt-4o-mini-transcribe".to_string(),
            api_custom_model_name: String::new(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key_present: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveSettingsRequest {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key: Option<String>,
    pub clear_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelDescriptor {
    pub id: String,
    pub label: String,
    pub engine: String,
    pub downloaded: bool,
    pub size_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub model_id: String,
    pub downloaded: bool,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiModelDescriptor {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub supports_custom_name: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptResult {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub provider: String,
    pub model_id: String,
}
