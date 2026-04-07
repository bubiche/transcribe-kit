use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{
    delete_model, ensure_model_downloaded, get_settings, list_api_models, list_input_devices,
    list_local_models, preload_local_model, save_settings, ApiModelDescriptor, AppSettings,
    AudioInputDeviceDescriptor, HotkeyMode, LiveCaptureProfile, LocalModelDescriptor, ProviderMode,
    SaveSettingsRequest,
};

#[derive(Clone, Copy)]
pub struct SettingsFormState {
    pub provider_mode: RwSignal<ProviderMode>,
    pub local_model_id: RwSignal<String>,
    pub selected_input_device_id: RwSignal<Option<String>>,
    pub live_capture_profile: RwSignal<LiveCaptureProfile>,
    pub hotkey_mode: RwSignal<HotkeyMode>,
    pub hotkey_shortcut: RwSignal<String>,
    pub api_model_id: RwSignal<String>,
    pub api_custom_model_name: RwSignal<String>,
    pub api_base_url: RwSignal<String>,
    pub api_key_input: RwSignal<String>,
    pub clear_api_key: RwSignal<bool>,
    pub api_key_present: RwSignal<bool>,
    pub postprocess_model: RwSignal<String>,
}

impl SettingsFormState {
    pub fn new() -> Self {
        Self {
            provider_mode: RwSignal::new(ProviderMode::Local),
            local_model_id: RwSignal::new("whisper-base".to_string()),
            selected_input_device_id: RwSignal::new(None),
            live_capture_profile: RwSignal::new(LiveCaptureProfile::default()),
            hotkey_mode: RwSignal::new(HotkeyMode::PushToTalk),
            hotkey_shortcut: RwSignal::new("CmdOrCtrl+Shift+T".to_string()),
            api_model_id: RwSignal::new("gpt-4o-mini-transcribe".to_string()),
            api_custom_model_name: RwSignal::new(String::new()),
            api_base_url: RwSignal::new("https://api.openai.com/v1".to_string()),
            api_key_input: RwSignal::new(String::new()),
            clear_api_key: RwSignal::new(false),
            api_key_present: RwSignal::new(false),
            postprocess_model: RwSignal::new("gpt-4o-mini".to_string()),
        }
    }

    pub fn apply_settings(self, settings: AppSettings) {
        self.provider_mode.set(settings.provider_mode);
        self.local_model_id.set(settings.local_model_id);
        self.selected_input_device_id
            .set(settings.selected_input_device_id);
        self.live_capture_profile.set(settings.live_capture_profile);
        self.hotkey_mode.set(settings.hotkey_mode);
        self.hotkey_shortcut.set(settings.hotkey_shortcut);
        self.api_model_id.set(settings.api_model_id);
        self.api_custom_model_name
            .set(settings.api_custom_model_name);
        self.api_base_url.set(settings.api_base_url);
        self.api_key_present.set(settings.api_key_present);
        self.postprocess_model.set(settings.postprocess_model);
        self.api_key_input.set(String::new());
        self.clear_api_key.set(false);
    }

    pub fn build_save_request(self) -> SaveSettingsRequest {
        SaveSettingsRequest {
            provider_mode: self.provider_mode.get(),
            local_model_id: self.local_model_id.get(),
            selected_input_device_id: self.selected_input_device_id.get(),
            live_capture_profile: self.live_capture_profile.get(),
            hotkey_mode: self.hotkey_mode.get(),
            hotkey_shortcut: self.hotkey_shortcut.get(),
            api_model_id: self.api_model_id.get(),
            api_custom_model_name: self.api_custom_model_name.get(),
            api_base_url: self.api_base_url.get(),
            api_key: {
                let api_key = self.api_key_input.get();
                if api_key.trim().is_empty() {
                    None
                } else {
                    Some(api_key)
                }
            },
            clear_api_key: self.clear_api_key.get(),
            postprocess_model: self.postprocess_model.get(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct DownloadState {
    pub is_downloading: RwSignal<bool>,
    pub download_model_id: RwSignal<Option<String>>,
    pub downloaded_bytes: RwSignal<u64>,
    pub total_bytes: RwSignal<Option<u64>>,
    pub download_error: RwSignal<Option<String>>,
}

impl DownloadState {
    pub fn new() -> Self {
        Self {
            is_downloading: RwSignal::new(false),
            download_model_id: RwSignal::new(None),
            downloaded_bytes: RwSignal::new(0),
            total_bytes: RwSignal::new(None),
            download_error: RwSignal::new(None),
        }
    }

    pub fn progress_fraction(self) -> Signal<f64> {
        Signal::derive(move || {
            let total = self.total_bytes.get().unwrap_or(0);
            if total == 0 {
                return 0.0;
            }
            self.downloaded_bytes.get() as f64 / total as f64
        })
    }

    pub fn reset(self) {
        self.is_downloading.set(false);
        self.download_model_id.set(None);
        self.downloaded_bytes.set(0);
        self.total_bytes.set(None);
        self.download_error.set(None);
    }
}

#[derive(Clone, Copy)]
pub struct SettingsFeatureState {
    pub form: SettingsFormState,
    pub local_models: RwSignal<Vec<LocalModelDescriptor>>,
    pub input_devices: RwSignal<Vec<AudioInputDeviceDescriptor>>,
    pub api_models: RwSignal<Vec<ApiModelDescriptor>>,
    pub load_error: RwSignal<Option<String>>,
    pub save_feedback: RwSignal<Option<String>>,
    pub hotkey_registration_error: RwSignal<Option<String>>,
    pub is_loading: RwSignal<bool>,
    pub is_saving: RwSignal<bool>,
    pub download: DownloadState,
    pub warming_model_id: RwSignal<Option<String>>,
    pub warmed_model_id: RwSignal<Option<String>>,
}

impl SettingsFeatureState {
    pub fn new() -> Self {
        Self {
            form: SettingsFormState::new(),
            local_models: RwSignal::new(Vec::new()),
            input_devices: RwSignal::new(Vec::new()),
            api_models: RwSignal::new(Vec::new()),
            load_error: RwSignal::new(None),
            save_feedback: RwSignal::new(None),
            hotkey_registration_error: RwSignal::new(None),
            is_loading: RwSignal::new(true),
            is_saving: RwSignal::new(false),
            download: DownloadState::new(),
            warming_model_id: RwSignal::new(None),
            warmed_model_id: RwSignal::new(None),
        }
    }

    pub fn custom_api_selected(self) -> Signal<bool> {
        Signal::derive(move || self.form.api_model_id.get() == "custom")
    }

    pub fn active_api_model_label(self) -> Signal<String> {
        Signal::derive(move || {
            let selected_id = self.form.api_model_id.get();
            self.api_models
                .get()
                .into_iter()
                .find(|model| model.id == selected_id)
                .map(|model| model.label)
                .unwrap_or_else(|| "Unknown API model".to_string())
        })
    }

    pub fn load(self) {
        spawn_local(async move {
            self.is_loading.set(true);
            self.load_error.set(None);

            let local_result = list_local_models().await;
            let input_result = list_input_devices().await;
            let api_result = list_api_models().await;
            let settings_result = get_settings().await;

            let mut problems = Vec::new();

            match local_result {
                Ok(local) => self.local_models.set(local),
                Err(error) => problems.push(format!("local models: {error}")),
            }

            match input_result {
                Ok(input_devices) => self.input_devices.set(input_devices),
                Err(error) => problems.push(format!("input devices: {error}")),
            }

            match api_result {
                Ok(api_models) => self.api_models.set(api_models),
                Err(error) => problems.push(format!("API models: {error}")),
            }

            match settings_result {
                Ok(settings) => {
                    self.hotkey_registration_error
                        .set(settings.hotkey_registration_error.clone());
                    self.form.apply_settings(settings);
                }
                Err(error) => problems.push(format!("settings: {error}")),
            }

            if !problems.is_empty() {
                self.load_error.set(Some(problems.join(" | ")));
            }

            self.is_loading.set(false);
        });
    }

    pub fn save(self, on_saved: impl Fn() + 'static) {
        let request = self.form.build_save_request();

        spawn_local(async move {
            self.is_saving.set(true);
            self.save_feedback.set(None);

            match save_settings(request).await {
                Ok(settings) => {
                    self.hotkey_registration_error
                        .set(settings.hotkey_registration_error.clone());
                    self.form.apply_settings(settings);
                    self.save_feedback.set(Some(
                        "Settings saved. The recording hotkey stays registered even while the app is in the background.".to_string(),
                    ));
                    on_saved();
                }
                Err(error) => {
                    if let Ok(settings) = get_settings().await {
                        self.hotkey_registration_error
                            .set(settings.hotkey_registration_error.clone());
                    }
                    self.save_feedback.set(Some(error));
                }
            }

            self.is_saving.set(false);
        });
    }

    pub fn selected_model_downloaded(self) -> Signal<bool> {
        Signal::derive(move || {
            let model_id = self.form.local_model_id.get();
            self.local_models
                .get()
                .iter()
                .find(|m| m.id == model_id)
                .map(|m| m.downloaded)
                .unwrap_or(false)
        })
    }

    pub fn download_selected_model(self) {
        let model_id = self.form.local_model_id.get();

        if self.download.is_downloading.get() {
            return;
        }

        self.download.reset();
        self.download.is_downloading.set(true);
        self.download.download_model_id.set(Some(model_id.clone()));

        let download = self.download;
        spawn_local(async move {
            let result = ensure_model_downloaded(&model_id, move |progress| {
                download.downloaded_bytes.set(progress.downloaded_bytes);
                download.total_bytes.set(progress.total_bytes);
            })
            .await;

            download.is_downloading.set(false);
            download.download_model_id.set(None);

            match result {
                Ok(()) => {
                    if let Ok(models) = list_local_models().await {
                        self.local_models.set(models);
                    }
                }
                Err(error) => {
                    download.download_error.set(Some(error));
                }
            }
        });
    }

    pub fn delete_selected_model(self) {
        let model_id = self.form.local_model_id.get();

        spawn_local(async move {
            match delete_model(&model_id).await {
                Ok(()) => {
                    if let Ok(models) = list_local_models().await {
                        self.local_models.set(models);
                    }
                }
                Err(error) => {
                    self.download.download_error.set(Some(error));
                }
            }
        });
    }

    pub fn maybe_preload_selected_local_model(self) {
        if self.form.provider_mode.get_untracked() != ProviderMode::Local {
            return;
        }

        let model_id = self.form.local_model_id.get_untracked();
        let model_downloaded = self
            .local_models
            .get_untracked()
            .iter()
            .any(|model| model.id == model_id && model.downloaded);

        if !model_downloaded {
            return;
        }

        if self.warmed_model_id.get_untracked().as_deref() == Some(model_id.as_str())
            || self.warming_model_id.get_untracked().as_deref() == Some(model_id.as_str())
        {
            return;
        }

        self.warming_model_id.set(Some(model_id.clone()));

        spawn_local(async move {
            let result = preload_local_model(&model_id).await;
            self.warming_model_id.set(None);

            if result.is_ok() {
                self.warmed_model_id.set(Some(model_id));
            }
        });
    }
}
