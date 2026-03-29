use crate::models::{ApiModelDescriptor, LocalModelDescriptor};

#[tauri::command]
pub fn health_check() -> String {
    "ok".to_string()
}

#[tauri::command]
pub fn list_local_models() -> Vec<LocalModelDescriptor> {
    vec![
        LocalModelDescriptor {
            id: "whisper-base".to_string(),
            label: "Whisper Base".to_string(),
            engine: "whisper".to_string(),
        },
        LocalModelDescriptor {
            id: "parakeet-tdt-0.6b-v3".to_string(),
            label: "NVIDIA Parakeet TDT 0.6B v3".to_string(),
            engine: "parakeet".to_string(),
        },
    ]
}

#[tauri::command]
pub fn list_api_models() -> Vec<ApiModelDescriptor> {
    vec![ApiModelDescriptor {
        id: "gpt-4o-mini-transcribe".to_string(),
        label: "OpenAI GPT-4o mini Transcribe".to_string(),
        provider: "openai-compatible".to_string(),
    }]
}

