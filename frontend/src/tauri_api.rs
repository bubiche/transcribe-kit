use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export async function invoke(command, args) {
  return await window.__TAURI__.core.invoke(command, args ?? {});
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = invoke)]
    async fn tauri_invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue>;
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiModelDescriptor {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub supports_custom_name: bool,
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
