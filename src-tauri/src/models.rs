use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderMode {
    Local,
    Api,
}

impl Default for ProviderMode {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyMode {
    PushToTalk,
    Toggle,
}

impl Default for HotkeyMode {
    fn default() -> Self {
        Self::PushToTalk
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub selected_input_device_id: Option<String>,
    pub hotkey_mode: HotkeyMode,
    pub hotkey_shortcut: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key_present: bool,
    pub hotkey_registration_error: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            provider_mode: ProviderMode::Local,
            local_model_id: "whisper-base".to_string(),
            selected_input_device_id: None,
            hotkey_mode: HotkeyMode::PushToTalk,
            hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
            api_model_id: "gpt-4o-mini-transcribe".to_string(),
            api_custom_model_name: String::new(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key_present: false,
            hotkey_registration_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveSettingsRequest {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub selected_input_device_id: Option<String>,
    pub hotkey_mode: HotkeyMode,
    pub hotkey_shortcut: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key: Option<String>,
    pub clear_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioInputDeviceDescriptor {
    pub id: String,
    pub label: String,
    pub manufacturer: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate_hz: Option<u32>,
    pub is_default: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InputType {
    File,
    Live,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LiveRecordingState {
    Idle,
    Recording,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveRecordingStatus {
    pub state: LiveRecordingState,
    pub input_device_id: Option<String>,
    pub input_device_label: Option<String>,
    pub output_file_path: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub channels: Option<u16>,
    pub duration_ms: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveRecordingResult {
    pub file_path: String,
    pub input_device_id: Option<String>,
    pub input_device_label: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionSource {
    pub provider: String,
    pub model_id: String,
    pub input_type: InputType,
    pub source_name: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptResult {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub source: TranscriptionSource,
    pub post_processed_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TranscriptionStreamEvent {
    Progress {
        progress_percent: i32,
    },
    Segment {
        segment_index: i32,
        segment: TranscriptSegment,
        accumulated_text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartFileTranscriptionRequest {
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscribeLiveRecordingRequest {
    pub file_path: String,
    pub input_device_id: Option<String>,
    pub input_device_label: String,
    pub duration_ms: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionJobState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionJobStatus {
    pub state: TranscriptionJobState,
    pub input_type: InputType,
    pub source_name: Option<String>,
    pub message: Option<String>,
}
