use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::{
    models::{AppSettings, ProviderMode, SaveSettingsRequest},
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
    #[error("{0}")]
    Validation(String),
    #[error("Could not access the system credential store: {0}")]
    Keyring(#[from] keyring::Error),
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    config_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSettings {
    provider_mode: ProviderMode,
    local_model_id: String,
    api_model_id: String,
    api_custom_model_name: String,
    api_base_url: String,
}

impl Default for StoredSettings {
    fn default() -> Self {
        let defaults = AppSettings::default();

        Self {
            provider_mode: defaults.provider_mode,
            local_model_id: defaults.local_model_id,
            api_model_id: defaults.api_model_id,
            api_custom_model_name: defaults.api_custom_model_name,
            api_base_url: defaults.api_base_url,
        }
    }
}

impl SettingsStore {
    pub fn new() -> Result<Self, SettingsError> {
        let project_dirs = ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit")
            .ok_or(SettingsError::MissingConfigDir)?;

        Ok(Self {
            config_path: project_dirs.config_dir().join("settings.json"),
        })
    }

    #[cfg(test)]
    fn with_path(path: PathBuf) -> Self {
        Self { config_path: path }
    }

    pub fn load(&self) -> Result<AppSettings, SettingsError> {
        let stored = self.read_stored_settings()?;
        let api_key_present = self.has_api_key(&stored.api_base_url)?;

        Ok(AppSettings {
            provider_mode: stored.provider_mode,
            local_model_id: stored.local_model_id,
            api_model_id: stored.api_model_id,
            api_custom_model_name: stored.api_custom_model_name,
            api_base_url: stored.api_base_url,
            api_key_present,
        })
    }

    pub fn save(
        &self,
        request: SaveSettingsRequest,
        local_model_ids: &[&str],
        api_model_ids: &[&str],
    ) -> Result<AppSettings, SettingsError> {
        validate_settings(&request, local_model_ids, api_model_ids)?;

        let stored = StoredSettings {
            provider_mode: request.provider_mode,
            local_model_id: request.local_model_id,
            api_model_id: request.api_model_id,
            api_custom_model_name: request.api_custom_model_name,
            api_base_url: normalize_base_url(&request.api_base_url),
        };

        if request.clear_api_key {
            self.delete_api_key(&stored.api_base_url)?;
        }

        if let Some(api_key) = request
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.set_api_key(&stored.api_base_url, api_key)?;
        }

        if matches!(stored.provider_mode, ProviderMode::Api)
            && !self.has_api_key(&stored.api_base_url)?
        {
            return Err(SettingsError::Validation(
                "An API key is required when API transcription is selected.".to_string(),
            ));
        }

        self.write_stored_settings(&stored)?;

        self.load()
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
            serde_json::to_string_pretty(settings).expect("stored settings serialization is valid");
        fs::write(&self.config_path, contents).map_err(SettingsError::WriteFile)
    }

    fn has_api_key(&self, base_url: &str) -> Result<bool, SettingsError> {
        match self.entry(base_url).get_password() {
            Ok(password) => Ok(!password.trim().is_empty()),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(SettingsError::Keyring(error)),
        }
    }

    fn set_api_key(&self, base_url: &str, api_key: &str) -> Result<(), SettingsError> {
        self.entry(base_url).set_password(api_key)?;
        Ok(())
    }

    fn delete_api_key(&self, base_url: &str) -> Result<(), SettingsError> {
        match self.entry(base_url).delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SettingsError::Keyring(error)),
        }
    }

    fn entry(&self, base_url: &str) -> Entry {
        Entry::new(KEYCHAIN_SERVICE, &credential_account(base_url)).expect("keyring entry")
    }
}

fn validate_settings(
    request: &SaveSettingsRequest,
    local_model_ids: &[&str],
    api_model_ids: &[&str],
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

    Ok(())
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn credential_account(base_url: &str) -> String {
    format!("openai-compatible::{}", normalize_base_url(base_url))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn temp_store() -> (TempDir, SettingsStore) {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = SettingsStore::with_path(temp_dir.path().join("settings.json"));
        (temp_dir, store)
    }

    #[test]
    fn load_defaults_when_no_settings_exist() {
        let (_temp_dir, store) = temp_store();

        let settings = store.load().expect("load settings");

        assert_eq!(settings.provider_mode, ProviderMode::Local);
        assert_eq!(settings.local_model_id, "whisper-base");
        assert_eq!(settings.api_model_id, "gpt-4o-mini-transcribe");
        assert!(!settings.api_key_present);
    }

    #[test]
    fn rejects_unknown_model_ids() {
        let (_temp_dir, store) = temp_store();

        let result = store.save(
            SaveSettingsRequest {
                provider_mode: ProviderMode::Local,
                local_model_id: "unknown".to_string(),
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: None,
                clear_api_key: false,
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
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
                api_model_id: "custom".to_string(),
                api_custom_model_name: "  ".to_string(),
                api_base_url: "https://api.openai.com/v1".to_string(),
                api_key: Some("secret".to_string()),
                clear_api_key: false,
            },
            &["whisper-base"],
            &["gpt-4o-mini-transcribe", "custom"],
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
                api_model_id: "gpt-4o-mini-transcribe".to_string(),
                api_custom_model_name: String::new(),
                api_base_url: "https://api.openai.com/v1".to_string(),
            })
            .expect("write settings");

        assert!(store.config_path.exists());
    }
}
