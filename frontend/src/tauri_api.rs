use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen(inline_js = r#"
export async function invoke(command, args) {
  return await window.__TAURI__.core.invoke(command, args ?? {});
}

export async function pickAudioFile() {
  const result = await window.__TAURI__.dialog.open({
    multiple: false,
    filters: [
      {
        name: 'Audio',
        extensions: ['wav', 'mp3', 'flac', 'ogg', 'm4a']
      }
    ]
  });

  if (Array.isArray(result)) {
    return result[0] ?? null;
  }

  return result ?? null;
}

export async function downloadModel(modelId, onProgress) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = onProgress;
  return await window.__TAURI__.core.invoke('ensure_model_downloaded', {
    modelId,
    onProgress: channel,
  });
}

export async function downloadLlmModel(modelId, onProgress) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = onProgress;
  return await window.__TAURI__.core.invoke('ensure_llm_model_downloaded', {
    modelId,
    onProgress: channel,
  });
}

export async function transcribeFile(filePath, onUpdate) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = onUpdate;
  return await window.__TAURI__.core.invoke('start_file_transcription', {
    request: {
      file_path: filePath,
    },
    onUpdate: channel,
  });
}

export async function transcribeLiveRecording(request, onUpdate) {
  const channel = new window.__TAURI__.core.Channel();
  channel.onmessage = onUpdate;
  return await window.__TAURI__.core.invoke('transcribe_live_recording', {
    request,
    onUpdate: channel,
  });
}

export async function pickSaveFile(defaultName) {
  const result = await window.__TAURI__.dialog.save({
    defaultPath: defaultName,
    filters: [
      {
        name: 'Text',
        extensions: ['txt']
      }
    ]
  });

  return result ?? null;
}

export async function writeTextFile(path, content) {
  return await window.__TAURI__.core.invoke('write_text_file', { path, content });
}

export async function writeClipboardText(text) {
  if (navigator?.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand('copy');
  document.body.removeChild(textarea);
  if (!copied) {
    throw new Error('Clipboard copy failed');
  }
}

export async function listenToAppEvent(eventName, onEvent) {
  return await window.__TAURI__.event.listen(eventName, (event) => {
    onEvent(event.payload);
  });
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = invoke)]
    async fn tauri_invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = pickAudioFile)]
    async fn pick_audio_file_js() -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = downloadModel)]
    async fn download_model_js(
        model_id: &str,
        on_progress: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = downloadLlmModel)]
    async fn download_llm_model_js(
        model_id: &str,
        on_progress: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = transcribeFile)]
    async fn transcribe_file_js(
        file_path: &str,
        on_update: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = transcribeLiveRecording)]
    async fn transcribe_live_recording_js(
        request: JsValue,
        on_update: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = pickSaveFile)]
    async fn pick_save_file_js(default_name: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = writeTextFile)]
    async fn write_text_file_js(path: &str, content: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = writeClipboardText)]
    async fn write_clipboard_text_js(text: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = listenToAppEvent)]
    async fn listen_to_app_event_js(
        event_name: &str,
        on_event: &Closure<dyn Fn(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderMode {
    Local,
    Api,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PostprocessProviderMode {
    #[default]
    Api,
    LocalLlm,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LiveCaptureProfile {
    #[default]
    MicrophoneOnly,
    MeetingMix,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyActivityState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeyActivityEvent {
    pub shortcut: String,
    pub mode: HotkeyMode,
    pub state: HotkeyActivityState,
    pub triggered_while_background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub selected_input_device_id: Option<String>,
    pub live_capture_profile: LiveCaptureProfile,
    pub hotkey_mode: HotkeyMode,
    pub hotkey_shortcut: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key_present: bool,
    #[serde(default)]
    pub api_key_insecure: bool,
    pub hotkey_registration_error: Option<String>,
    pub postprocess_model: String,
    pub postprocess_provider_mode: PostprocessProviderMode,
    pub local_llm_model_id: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            provider_mode: ProviderMode::Local,
            local_model_id: "whisper-base".to_string(),
            selected_input_device_id: None,
            live_capture_profile: LiveCaptureProfile::default(),
            hotkey_mode: HotkeyMode::PushToTalk,
            hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
            api_model_id: "gpt-4o-mini-transcribe".to_string(),
            api_custom_model_name: String::new(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            api_key_present: false,
            api_key_insecure: false,
            hotkey_registration_error: None,
            postprocess_model: "gpt-4o-mini".to_string(),
            postprocess_provider_mode: PostprocessProviderMode::Api,
            local_llm_model_id: "llm-qwen-3.5-0.8b".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveSettingsRequest {
    pub provider_mode: ProviderMode,
    pub local_model_id: String,
    pub selected_input_device_id: Option<String>,
    pub live_capture_profile: LiveCaptureProfile,
    pub hotkey_mode: HotkeyMode,
    pub hotkey_shortcut: String,
    pub api_model_id: String,
    pub api_custom_model_name: String,
    pub api_base_url: String,
    pub api_key: Option<String>,
    pub clear_api_key: bool,
    pub postprocess_model: String,
    pub postprocess_provider_mode: PostprocessProviderMode,
    pub local_llm_model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioInputDeviceDescriptor {
    pub id: String,
    pub label: String,
    pub manufacturer: Option<String>,
    pub channels: Option<u16>,
    pub sample_rate_hz: Option<u32>,
    pub is_default: bool,
    #[serde(default)]
    pub is_output_loopback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalModelDescriptor {
    pub id: String,
    pub label: String,
    pub engine: String,
    pub downloaded: bool,
    pub size_label: String,
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
pub struct PostProcessTemplate {
    pub id: String,
    pub name: String,
    pub prompt: String,
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
    #[serde(default)]
    pub is_dual_capture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscribeLiveRecordingRequest {
    pub file_path: String,
    pub input_device_id: Option<String>,
    pub input_device_label: String,
    #[serde(default)]
    pub live_capture_profile: LiveCaptureProfile,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionSource {
    pub provider: String,
    pub model_id: String,
    pub input_type: InputType,
    #[serde(default)]
    pub live_capture_profile: Option<LiveCaptureProfile>,
    pub source_name: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionJobState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionJobStatus {
    pub state: TranscriptionJobState,
    pub input_type: InputType,
    pub source_name: Option<String>,
    pub message: Option<String>,
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

pub async fn get_live_recording_status() -> Result<LiveRecordingStatus, String> {
    invoke_command("get_live_recording_status", ()).await
}

pub async fn list_local_models() -> Result<Vec<LocalModelDescriptor>, String> {
    invoke_command("list_local_models", ()).await
}

pub async fn list_input_devices() -> Result<Vec<AudioInputDeviceDescriptor>, String> {
    invoke_command("list_input_devices", ()).await
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

pub async fn list_templates() -> Result<Vec<PostProcessTemplate>, String> {
    invoke_command("list_templates", ()).await
}

pub async fn save_templates(templates: Vec<PostProcessTemplate>) -> Result<(), String> {
    invoke_command("save_templates", SaveTemplatesArgs { templates }).await
}

#[derive(Debug, Clone, Serialize)]
struct SaveTemplatesArgs {
    templates: Vec<PostProcessTemplate>,
}

pub async fn run_postprocess(
    transcript_text: String,
    template_id: String,
    enable_thinking: bool,
) -> Result<String, String> {
    invoke_command(
        "run_postprocess",
        RunPostprocessArgs {
            transcript_text,
            template_id,
            enable_thinking,
        },
    )
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunPostprocessArgs {
    transcript_text: String,
    template_id: String,
    enable_thinking: bool,
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

pub async fn preload_local_model(model_id: &str) -> Result<(), String> {
    invoke_command(
        "preload_local_model",
        ModelIdArg {
            model_id: model_id.to_string(),
        },
    )
    .await
}

pub async fn list_local_llm_models() -> Result<Vec<LocalModelDescriptor>, String> {
    invoke_command("list_local_llm_models", ()).await
}

pub async fn delete_llm_model(model_id: &str) -> Result<(), String> {
    invoke_command(
        "delete_llm_model",
        ModelIdArg {
            model_id: model_id.to_string(),
        },
    )
    .await
}

pub async fn ensure_llm_model_downloaded(
    model_id: &str,
    on_progress: impl Fn(ModelDownloadProgress) + 'static,
) -> Result<(), String> {
    let closure = Closure::wrap(Box::new(move |value: JsValue| {
        if let Some(progress) = parse_channel_message(&value) {
            on_progress(progress);
        }
    }) as Box<dyn Fn(JsValue)>);

    let result = download_llm_model_js(model_id, &closure)
        .await
        .map_err(js_error_message);

    closure.forget();
    result.map(|_| ())
}

pub async fn cancel_postprocess() -> Result<(), String> {
    invoke_command("cancel_postprocess", ()).await
}

pub async fn start_live_transcription() -> Result<LiveRecordingStatus, String> {
    invoke_command("start_live_transcription", ()).await
}

pub async fn stop_live_transcription() -> Result<LiveRecordingResult, String> {
    invoke_command("stop_live_transcription", ()).await
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

    let done = get("done").and_then(|v| v.as_bool()).unwrap_or(false);

    Some(ModelDownloadProgress {
        model_id,
        downloaded_bytes,
        total_bytes,
        done,
    })
}

pub async fn start_file_transcription(
    file_path: &str,
    on_update: impl Fn(TranscriptionStreamEvent) + 'static,
) -> Result<TranscriptResult, String> {
    let closure = Closure::wrap(Box::new(move |value: JsValue| {
        if let Some(event) = parse_transcription_stream_event(&value) {
            on_update(event);
        }
    }) as Box<dyn Fn(JsValue)>);

    let result = transcribe_file_js(file_path, &closure)
        .await
        .map_err(js_error_message);

    closure.forget();
    result
        .and_then(|value| serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string()))
}

pub async fn transcribe_live_recording(
    request: TranscribeLiveRecordingRequest,
    on_update: impl Fn(TranscriptionStreamEvent) + 'static,
) -> Result<TranscriptResult, String> {
    let request = serde_wasm_bindgen::to_value(&request).map_err(js_error_message)?;
    let closure = Closure::wrap(Box::new(move |value: JsValue| {
        if let Some(event) = parse_transcription_stream_event(&value) {
            on_update(event);
        }
    }) as Box<dyn Fn(JsValue)>);

    let result = transcribe_live_recording_js(request, &closure)
        .await
        .map_err(js_error_message);

    closure.forget();
    result
        .and_then(|value| serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string()))
}

pub async fn pick_save_file(default_name: &str) -> Result<Option<String>, String> {
    let value = pick_save_file_js(default_name)
        .await
        .map_err(js_error_message)?;

    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }

    value
        .as_string()
        .map(Some)
        .ok_or_else(|| "Save dialog returned an unexpected value".to_string())
}

pub async fn write_text_file(path: &str, content: &str) -> Result<(), String> {
    write_text_file_js(path, content)
        .await
        .map_err(js_error_message)
        .map(|_| ())
}

pub async fn write_clipboard_text(text: &str) -> Result<(), String> {
    write_clipboard_text_js(text)
        .await
        .map_err(js_error_message)
        .map(|_| ())
}

pub async fn listen_to_app_event(
    event_name: &str,
    on_event: impl Fn(JsValue) + 'static,
) -> Result<Box<dyn FnOnce()>, String> {
    let closure = Closure::wrap(Box::new(move |value: JsValue| {
        on_event(value);
    }) as Box<dyn Fn(JsValue)>);

    let unlisten = listen_to_app_event_js(event_name, &closure)
        .await
        .map_err(js_error_message)?;

    closure.forget();

    let unlisten_fn = unlisten
        .dyn_into::<js_sys::Function>()
        .map_err(|_| "Tauri event listener returned an unexpected value".to_string())?;

    Ok(Box::new(move || {
        let _ = unlisten_fn.call0(&JsValue::NULL);
    }))
}

pub async fn pick_audio_file() -> Result<Option<String>, String> {
    let value = pick_audio_file_js().await.map_err(js_error_message)?;

    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }

    value
        .as_string()
        .map(Some)
        .ok_or_else(|| "File picker returned an unexpected value".to_string())
}

fn parse_transcription_stream_event(value: &JsValue) -> Option<TranscriptionStreamEvent> {
    let get =
        |target: &JsValue, key: &str| js_sys::Reflect::get(target, &JsValue::from_str(key)).ok();
    let get_string = |target: &JsValue, snake: &str, camel: &str| {
        get(target, snake)
            .or_else(|| get(target, camel))
            .and_then(|v| v.as_string())
    };
    let get_i64 = |target: &JsValue, snake: &str, camel: &str| {
        get(target, snake)
            .or_else(|| get(target, camel))
            .and_then(|v| v.as_f64())
            .map(|v| v as i64)
    };
    let get_i32 = |target: &JsValue, snake: &str, camel: &str| {
        get(target, snake)
            .or_else(|| get(target, camel))
            .and_then(|v| v.as_f64())
            .map(|v| v as i32)
    };

    match get_string(value, "kind", "kind")?.as_str() {
        "progress" => Some(TranscriptionStreamEvent::Progress {
            progress_percent: get_i32(value, "progress_percent", "progressPercent")?,
        }),
        "segment" => {
            let segment_value = get(value, "segment")?;
            Some(TranscriptionStreamEvent::Segment {
                segment_index: get_i32(value, "segment_index", "segmentIndex")?,
                segment: TranscriptSegment {
                    start_ms: get_i64(&segment_value, "start_ms", "startMs")?,
                    end_ms: get_i64(&segment_value, "end_ms", "endMs")?,
                    text: get_string(&segment_value, "text", "text")?,
                },
                accumulated_text: get_string(value, "accumulated_text", "accumulatedText")?,
            })
        }
        _ => None,
    }
}

pub const AUDIO_LEVEL_EVENT_NAME: &str = "transcribe-kit://audio-level";

#[derive(Debug, Clone, Deserialize)]
pub struct AudioLevelEvent {
    pub rms: f32,
    #[allow(dead_code)]
    pub peak: f32,
}

pub async fn start_audio_monitor(device_id: Option<String>) -> Result<(), String> {
    invoke_command("start_audio_monitor", StartAudioMonitorArgs { device_id }).await
}

pub async fn stop_audio_monitor() -> Result<(), String> {
    invoke_command("stop_audio_monitor", ()).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartAudioMonitorArgs {
    device_id: Option<String>,
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
