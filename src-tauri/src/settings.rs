use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::{
    hotkeys,
    models::{AppSettings, HotkeyMode, LiveCaptureProfile, ProviderMode, SaveSettingsRequest},
    providers::api_openai_compatible::ApiCredentials,
};

const KEYCHAIN_SERVICE: &str = "dev.transcribekit.desktop";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("Transcribe Kit could not determine a settings directory on this system.")]
    MissingConfigDir,
    #[error("Could not create the settings directory: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("Could not read the settings file: {0}")]
    ReadFile(#[source] std::io::Error),
    #[error("Could not parse the settings file: {0}")]
    ParseFile(#[source] serde_json::Error),
    #[error("Could not write the settings file: {0}")]
    WriteFile(#[source] std::io::Error),
    #[error("Could not serialize settings: {0}")]
    SerializeFile(#[source] serde_json::Error),
    #[error("{0}")]
    Validation(String),
    #[error("Could not access the system credential store: {0}")]
    Keyring(#[from] keyring::Error),
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    config_path: PathBuf,
    keyring_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSettings {
    provider_mode: ProviderMode,
    local_model_id: String,
    selected_input_device_id: Option<String>,
    #[serde(default)]
    live_capture_profile: LiveCaptureProfile,
    #[serde(default = "default_hotkey_mode")]
    pub(crate) hotkey_mode: HotkeyMode,
    #[serde(default = "default_hotkey_shortcut")]
    pub(crate) hotkey_shortcut: String,
    api_model_id: String,
    api_custom_model_name: String,
    api_base_url: String,
    #[serde(default = "default_postprocess_model")]
    postprocess_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key_plaintext: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedSettingsSave {
    stored: StoredSettings,
    api_key: Option<String>,
    clear_api_key: bool,
}

impl PreparedSettingsSave {
    pub(crate) fn hotkey_shortcut(&self) -> &str {
        &self.stored.hotkey_shortcut
    }

    pub(crate) fn hotkey_mode(&self) -> HotkeyMode {
        self.stored.hotkey_mode
    }
}

impl Default for StoredSettings {
    fn default() -> Self {
        let defaults = AppSettings::default();

        Self {
            provider_mode: defaults.provider_mode,
            local_model_id: defaults.local_model_id,
            selected_input_device_id: defaults.selected_input_device_id,
            live_capture_profile: defaults.live_capture_profile,
            hotkey_mode: defaults.hotkey_mode,
            hotkey_shortcut: defaults.hotkey_shortcut,
            api_model_id: defaults.api_model_id,
            api_custom_model_name: defaults.api_custom_model_name,
            api_base_url: defaults.api_base_url,
            postprocess_model: defaults.postprocess_model,
            api_key_plaintext: None,
        }
    }
}

impl SettingsStore {
    pub fn new() -> Result<Self, SettingsError> {
        let project_dirs = ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit")
            .ok_or(SettingsError::MissingConfigDir)?;

        let keyring_available = Entry::new(KEYCHAIN_SERVICE, "probe")
            .and_then(|entry| {
                entry.get_password().or_else(|e| match e {
                    keyring::Error::NoEntry => Ok(String::new()),
                    other => Err(other),
                })
            })
            .is_ok();

        if !keyring_available {
            eprintln!(
                "System keyring is not available. API keys will be stored in the settings file \
                 (unencrypted). Install a keyring service for secure storage."
            );
        }

        Ok(Self {
            config_path: project_dirs.config_dir().join("settings.json"),
            keyring_available,
        })
    }

    #[cfg(test)]
    fn with_path(path: PathBuf) -> Self {
        Self {
            config_path: path,
            keyring_available: false,
        }
    }

    pub fn load(&self) -> Result<AppSettings, SettingsError> {
        let stored = self.read_stored_settings()?;
        let api_key_present = self.has_api_key(&stored)?;

        Ok(AppSettings {
            provider_mode: stored.provider_mode,
            local_model_id: stored.local_model_id,
            selected_input_device_id: stored.selected_input_device_id,
            live_capture_profile: stored.live_capture_profile,
            hotkey_mode: stored.hotkey_mode,
            hotkey_shortcut: stored.hotkey_shortcut,
            api_model_id: stored.api_model_id,
            api_custom_model_name: stored.api_custom_model_name,
            api_base_url: stored.api_base_url,
            api_key_present,
            api_key_insecure: !self.keyring_available && api_key_present,
            hotkey_registration_error: None,
            postprocess_model: stored.postprocess_model,
        })
    }

    pub(crate) fn prepare_save(
        &self,
        request: SaveSettingsRequest,
        local_model_ids: &[&str],
        api_model_ids: &[&str],
        input_device_ids: &[String],
    ) -> Result<PreparedSettingsSave, SettingsError> {
        validate_settings(&request, local_model_ids, api_model_ids, input_device_ids)?;

        // Preserve existing plaintext key from file if present
        let existing_plaintext = self.read_stored_settings()?.api_key_plaintext;

        let stored = StoredSettings {
            provider_mode: request.provider_mode,
            local_model_id: request.local_model_id,
            selected_input_device_id: normalize_input_device_id(
                request.selected_input_device_id.as_deref(),
            ),
            live_capture_profile: request.live_capture_profile,
            hotkey_mode: request.hotkey_mode,
            hotkey_shortcut: hotkeys::validate_shortcut(&request.hotkey_shortcut)
                .map_err(SettingsError::Validation)?,
            api_model_id: request.api_model_id,
            api_custom_model_name: request.api_custom_model_name,
            api_base_url: normalize_base_url(&request.api_base_url),
            postprocess_model: request.postprocess_model,
            api_key_plaintext: existing_plaintext,
        };

        let api_key = request
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        if matches!(stored.provider_mode, ProviderMode::Api)
            && api_key.is_none()
            && (request.clear_api_key || !self.has_api_key(&stored)?)
        {
            return Err(SettingsError::Validation(
                "An API key is required when API transcription is selected.".to_string(),
            ));
        }

        Ok(PreparedSettingsSave {
            stored,
            api_key,
            clear_api_key: request.clear_api_key,
        })
    }

    pub(crate) fn commit_save(
        &self,
        prepared: PreparedSettingsSave,
    ) -> Result<AppSettings, SettingsError> {
        let mut stored = prepared.stored;

        if prepared.clear_api_key {
            self.delete_api_key(&stored.api_base_url)?;
            if !self.keyring_available {
                stored.api_key_plaintext = None;
            }
        }

        if let Some(api_key) = prepared.api_key.as_deref() {
            self.set_api_key(&stored.api_base_url, api_key)?;
            if !self.keyring_available {
                stored.api_key_plaintext = Some(api_key.to_string());
            }
        }

        // When keyring IS available, never persist the key in the file
        if self.keyring_available {
            stored.api_key_plaintext = None;
        }

        self.write_stored_settings(&stored)?;

        self.load()
    }

    #[allow(dead_code)]
    pub fn save(
        &self,
        request: SaveSettingsRequest,
        local_model_ids: &[&str],
        api_model_ids: &[&str],
        input_device_ids: &[String],
    ) -> Result<AppSettings, SettingsError> {
        let prepared =
            self.prepare_save(request, local_model_ids, api_model_ids, input_device_ids)?;
        self.commit_save(prepared)
    }

    fn read_stored_settings(&self) -> Result<StoredSettings, SettingsError> {
        match fs::read_to_string(&self.config_path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(SettingsError::ParseFile),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredSettings::default())
            }
            Err(error) => Err(SettingsError::ReadFile(error)),
        }
    }

    fn write_stored_settings(&self, settings: &StoredSettings) -> Result<(), SettingsError> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(SettingsError::CreateDirectory)?;
        }

        let contents =
            serde_json::to_string_pretty(settings).map_err(SettingsError::SerializeFile)?;
        fs::write(&self.config_path, contents).map_err(SettingsError::WriteFile)
    }

    fn has_api_key(&self, stored: &StoredSettings) -> Result<bool, SettingsError> {
        if self.keyring_available {
            if let Some(entry) = self.entry(&stored.api_base_url) {
                return match entry.get_password() {
                    Ok(password) => Ok(!password.trim().is_empty()),
                    Err(keyring::Error::NoEntry) => Ok(false),
                    Err(error) => Err(SettingsError::Keyring(error)),
                };
            }
        }
        // File fallback
        Ok(stored
            .api_key_plaintext
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty()))
    }

    pub fn get_api_key(&self, base_url: &str) -> Result<String, SettingsError> {
        if self.keyring_available {
            if let Some(entry) = self.entry(base_url) {
                return map_retrieved_api_key(entry.get_password());
            }
        }
        // File fallback
        let stored = self.read_stored_settings()?;
        match stored.api_key_plaintext {
            Some(key) if !key.trim().is_empty() => Ok(key.trim().to_string()),
            _ => Err(SettingsError::Validation(
                "No API key is stored for the configured API base URL.".to_string(),
            )),
        }
    }

    fn set_api_key(&self, base_url: &str, api_key: &str) -> Result<(), SettingsError> {
        if self.keyring_available {
            if let Some(entry) = self.entry(base_url) {
                entry.set_password(api_key)?;
                return Ok(());
            }
        }
        // File fallback — written as part of StoredSettings in commit_save
        Ok(())
    }

    fn delete_api_key(&self, base_url: &str) -> Result<(), SettingsError> {
        if self.keyring_available {
            if let Some(entry) = self.entry(base_url) {
                match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => return Ok(()),
                    Err(error) => return Err(SettingsError::Keyring(error)),
                }
            }
        }
        Ok(())
    }

    fn entry(&self, base_url: &str) -> Option<Entry> {
        Entry::new(KEYCHAIN_SERVICE, &credential_account(base_url)).ok()
    }
}

fn validate_settings(
    request: &SaveSettingsRequest,
    local_model_ids: &[&str],
    api_model_ids: &[&str],
    input_device_ids: &[String],
) -> Result<(), SettingsError> {
    if !local_model_ids.contains(&request.local_model_id.as_str()) {
        return Err(SettingsError::Validation(
            "Select a supported local Whisper model.".to_string(),
        ));
    }

    if !api_model_ids.contains(&request.api_model_id.as_str()) {
        return Err(SettingsError::Validation(
            "Select a supported API transcription model option.".to_string(),
        ));
    }

    let normalized_base_url = normalize_base_url(&request.api_base_url);

    if matches!(request.provider_mode, ProviderMode::Api) {
        ApiCredentials {
            api_key: request
                .api_key
                .clone()
                .unwrap_or_else(|| "stored-in-keychain".to_string()),
            base_url: normalized_base_url.clone(),
        }
        .validate()
        .map_err(|message| SettingsError::Validation(message.to_string()))?;
    } else if !normalized_base_url.is_empty() {
        ApiCredentials {
            api_key: "not-used".to_string(),
            base_url: normalized_base_url.clone(),
        }
        .validate()
        .map_err(|message| SettingsError::Validation(message.to_string()))?;
    }

    if request.api_model_id == "custom" && request.api_custom_model_name.trim().is_empty() {
        return Err(SettingsError::Validation(
            "Enter a model name for the custom API option.".to_string(),
        ));
    }

    if let Some(selected_input_device_id) =
        normalize_input_device_id(request.selected_input_device_id.as_deref())
    {
        if !input_device_ids
            .iter()
            .any(|device_id| device_id == &selected_input_device_id)
        {
            return Err(SettingsError::Validation(
                "Select an available audio input or switch back to System default.".to_string(),
            ));
        }
    }

    Ok(())
}

fn default_postprocess_model() -> String {
    AppSettings::default().postprocess_model
}

fn default_hotkey_mode() -> HotkeyMode {
    AppSettings::default().hotkey_mode
}

fn default_hotkey_shortcut() -> String {
    AppSettings::default().hotkey_shortcut
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn normalize_input_device_id(device_id: Option<&str>) -> Option<String> {
    device_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn credential_account(base_url: &str) -> String {
    format!("openai-compatible::{}", normalize_base_url(base_url))
}

fn map_retrieved_api_key(result: Result<String, keyring::Error>) -> Result<String, SettingsError> {
    match result {
        Ok(password) if password.trim().is_empty() => Err(SettingsError::Validation(
            "No API key is stored for the configured API base URL.".to_string(),
        )),
        Ok(password) => Ok(password.trim().to_string()),
        Err(keyring::Error::NoEntry) => Err(SettingsError::Validation(
            "No API key is stored for the configured API base URL.".to_string(),
        )),
        Err(error) => Err(SettingsError::Keyring(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use tempfile::TempDir;

    use super::*;

    fn temp_store() -> (TempDir, SettingsStore) {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = SettingsStore::with_path(temp_dir.path().join("settings.json"));
        (temp_dir, store)
    }

    fn unique_base_url(label: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        format!("https://{label}-{nonce}.example.invalid")
    }

    #[test]
    fn load_defaults_when_no_settings_exist() {
        let (_temp_dir, store) = temp_store();

        let settings = store.load().expect("load settings");

        assert_eq!(settings.provider_mode, ProviderMode::Local);
        assert_eq!(settings.local_model_id, "whisper-base");
        assert_eq!(settings.selected_input_device_id, None);
        assert_eq!(
            settings.live_capture_profile,
            LiveCaptureProfile::MicrophoneOnly
        );
        assert_eq!(settings.hotkey_mode, HotkeyMode::PushToTalk);
        assert_eq!(settings.hotkey_shortcut, "CmdOrCtrl+Shift+T");
        assert_eq!(settings.api_model_id, "gpt-4o-mini-transcribe");
        // api_key_present is derived from the system keyring, not stored
        // settings, so it depends on external state and is not asserted here.
        assert_eq!(settings.hotkey_registration_error, None);
    }

    #[test]
    fn rejects_unknown_model_ids() {
        let (_temp_dir, store) = temp_store();

        let result = store.save(
            SaveSettingsRequest {
                provider_mode: ProviderMode::Local,
                local_model_id: "unknown".to_string(),
                selected_input_device_id: None,
                live_capture_profile: LiveCaptureProfile::default(),
                hotkey_mode: HotkeyMode::PushToTalk,
                hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: None,
                clear_api_key: false,
                postprocess_model: "gpt-4o-mini".to_string(),
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
            &[],
        );

        assert!(matches!(result, Err(SettingsError::Validation(_))));
    }

    #[test]
    fn custom_api_model_requires_name() {
        let (_temp_dir, store) = temp_store();

        let result = store.save(
            SaveSettingsRequest {
                provider_mode: ProviderMode::Api,
                local_model_id: "whisper-base".to_string(),
                selected_input_device_id: None,
                live_capture_profile: LiveCaptureProfile::default(),
                hotkey_mode: HotkeyMode::PushToTalk,
                hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
                api_model_id: "custom".to_string(),
                api_custom_model_name: "  ".to_string(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: Some("secret".to_string()),
                clear_api_key: false,
                postprocess_model: "gpt-4o-mini".to_string(),
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
            &[],
        );

        assert!(matches!(result, Err(SettingsError::Validation(_))));
    }

    #[test]
    fn normalize_base_url_trims_trailing_slash() {
        assert_eq!(
            normalize_base_url(" https://api.openai.com/v1/ "),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn writes_settings_file() {
        let (_temp_dir, store) = temp_store();

        store
            .write_stored_settings(&StoredSettings {
                provider_mode: ProviderMode::Local,
                local_model_id: "whisper-base".to_string(),
                selected_input_device_id: None,
                live_capture_profile: LiveCaptureProfile::default(),
                hotkey_mode: HotkeyMode::PushToTalk,
                hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                postprocess_model: "gpt-4o-mini".to_string(),
                api_key_plaintext: None,
            })
            .expect("write settings");

        assert!(store.config_path.exists());
    }

    #[test]
    fn rejects_unknown_selected_input_device_id() {
        let (_temp_dir, store) = temp_store();

        let result = store.save(
            SaveSettingsRequest {
                provider_mode: ProviderMode::Local,
                local_model_id: "whisper-base".to_string(),
                selected_input_device_id: Some("missing-device".to_string()),
                live_capture_profile: LiveCaptureProfile::default(),
                hotkey_mode: HotkeyMode::PushToTalk,
                hotkey_shortcut: "CmdOrCtrl+Shift+T".to_string(),
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: None,
                clear_api_key: false,
                postprocess_model: "gpt-4o-mini".to_string(),
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
            &["available-device".to_string()],
        );

        assert!(matches!(result, Err(SettingsError::Validation(_))));
    }

    #[test]
    fn rejects_invalid_hotkeys() {
        let (_temp_dir, store) = temp_store();

        let result = store.save(
            SaveSettingsRequest {
                provider_mode: ProviderMode::Local,
                local_model_id: "whisper-base".to_string(),
                selected_input_device_id: None,
                live_capture_profile: LiveCaptureProfile::default(),
                hotkey_mode: HotkeyMode::PushToTalk,
                hotkey_shortcut: "T".to_string(),
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: None,
                clear_api_key: false,
                postprocess_model: "gpt-4o-mini".to_string(),
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
            &[],
        );

        assert!(matches!(result, Err(SettingsError::Validation(_))));
    }

    #[test]
    fn get_api_key_returns_validation_error_when_missing() {
        let (_temp_dir, store) = temp_store();
        let base_url = unique_base_url("missing-key");

        let result = store.get_api_key(&base_url);

        match result {
            Err(SettingsError::Validation(message)) => {
                assert!(message.contains("No API key is stored"));
            }
            other => panic!("expected missing-key validation error, got {other:?}"),
        }
    }

    #[test]
    fn get_api_key_mapping_returns_stored_value() {
        let stored_key =
            map_retrieved_api_key(Ok("super-secret".to_string())).expect("read stored key");
        assert_eq!(stored_key, "super-secret");
    }

    #[test]
    fn get_api_key_mapping_trims_stored_value() {
        let stored_key =
            map_retrieved_api_key(Ok("  super-secret  ".to_string())).expect("read stored key");
        assert_eq!(stored_key, "super-secret");
    }

    #[test]
    fn get_api_key_mapping_preserves_keyring_errors() {
        let result = map_retrieved_api_key(Err(keyring::Error::Invalid(
            "account".to_string(),
            "invalid".to_string(),
        )));

        match result {
            Err(SettingsError::Keyring(keyring::Error::Invalid(account, reason))) => {
                assert_eq!(account, "account");
                assert_eq!(reason, "invalid");
            }
            other => panic!("expected keyring invalid error, got {other:?}"),
        }
    }

    #[test]
    fn existing_settings_without_postprocess_model_deserialize_with_default() {
        let (_temp_dir, store) = temp_store();

        // Simulate a settings file written before postprocess_model existed
        let legacy_json = serde_json::json!({
            "provider_mode": "local",
            "local_model_id": "whisper-base",
            "selected_input_device_id": null,
            "live_capture_profile": "microphone-only",
            "hotkey_mode": "push-to-talk",
            "hotkey_shortcut": "CmdOrCtrl+Shift+T",
            "api_model_id": "gpt-4o-mini-transcribe",
            "api_custom_model_name": "",
            "api_base_url": "https://api.openai.com/v1"
        });
        fs::write(&store.config_path, legacy_json.to_string()).expect("write legacy settings");

        let stored: StoredSettings =
            serde_json::from_str(&fs::read_to_string(&store.config_path).unwrap()).unwrap();

        assert_eq!(stored.postprocess_model, "gpt-4o-mini");
    }
}
