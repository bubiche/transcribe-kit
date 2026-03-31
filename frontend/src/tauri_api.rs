use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export async function invoke(command, args) {
  return await window.__TAURI__.core.invoke(command, args ?? {});
}

export async function downloadModel(modelId, onProgress) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = onProgress;
  return await window.__TAURI__.core.invoke('ensure_model_downloaded', {
    modelId,
    onProgress: channel,
  });
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = invoke)]
    async fn tauri_invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = downloadModel)]
    async fn download_model_js(
        model_id: &str,
        on_progress: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalModelDescriptor {
    pub id: String,
    pub label: String,
    pub engine: String,
    pub downloaded: bool,
    pub size_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelStatus {
    pub model_id: String,
    pub downloaded: bool,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelDownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
pub struct TranscriptResult {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub provider: String,
    pub model_id: String,
}

pub async fn invoke_command<T>(command: &str, args: impl Serialize) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let args = serde_wasm_bindgen::to_value(&args).map_err(js_error_message)?;
    let value = tauri_invoke(command, args)
        .await
        .map_err(js_error_message)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn get_settings() -> Result<AppSettings, String> {
    invoke_command("get_settings", ()).await
}

pub async fn list_local_models() -> Result<Vec<LocalModelDescriptor>, String> {
    invoke_command("list_local_models", ()).await
}

pub async fn list_api_models() -> Result<Vec<ApiModelDescriptor>, String> {
    invoke_command("list_api_models", ()).await
}

pub async fn save_settings(request: SaveSettingsRequest) -> Result<AppSettings, String> {
    invoke_command("save_settings", SaveSettingsArgs { request }).await
}

#[derive(Debug, Clone, Serialize)]
struct SaveSettingsArgs {
    request: SaveSettingsRequest,
}

pub async fn get_model_status(model_id: &str) -> Result<ModelStatus, String> {
    invoke_command(
        "get_model_status",
        ModelIdArg {
            model_id: model_id.to_string(),
        },
    )
    .await
}

pub async fn delete_model(model_id: &str) -> Result<(), String> {
    invoke_command(
        "delete_model",
        ModelIdArg {
            model_id: model_id.to_string(),
        },
    )
    .await
}

pub async fn ensure_model_downloaded(
    model_id: &str,
    on_progress: impl Fn(ModelDownloadProgress) + 'static,
) -> Result<(), String> {
    let closure = Closure::wrap(Box::new(move |value: JsValue| {
        if let Some(progress) = parse_channel_message(&value) {
            on_progress(progress);
        }
    }) as Box<dyn Fn(JsValue)>);

    let result = download_model_js(model_id, &closure)
        .await
        .map_err(js_error_message);

    closure.forget();
    result.map(|_| ())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelIdArg {
    model_id: String,
}

fn parse_channel_message(value: &JsValue) -> Option<ModelDownloadProgress> {
    let get = |key: &str| js_sys::Reflect::get(value, &JsValue::from_str(key)).ok();

    let model_id = get("model_id")
        .or_else(|| get("modelId"))
        .and_then(|v| v.as_string())?;

    let downloaded_bytes = get("downloaded_bytes")
        .or_else(|| get("downloadedBytes"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u64;

    let total_bytes = get("total_bytes")
        .or_else(|| get("totalBytes"))
        .and_then(|v| v.as_f64())
        .map(|v| v as u64);

    let done = get("done")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Some(ModelDownloadProgress {
        model_id,
        downloaded_bytes,
        total_bytes,
        done,
    })
}

pub async fn start_file_transcription(file_path: &str) -> Result<TranscriptResult, String> {
    invoke_command(
        "start_file_transcription",
        StartFileTranscriptionArgs {
            file_path: file_path.to_string(),
        },
    )
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartFileTranscriptionArgs {
    file_path: String,
}

fn js_error_message(error: impl Into<JsValue>) -> String {
    let value = error.into();

    value
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&value, &JsValue::from_str("message"))
                .ok()?
                .as_string()
        })
        .unwrap_or_else(|| "Unexpected Tauri invocation error".to_string())
}
