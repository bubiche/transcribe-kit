use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::tauri_api::{AudioInputDeviceDescriptor, HotkeyMode, ProviderMode};

use super::state::{DownloadState, SettingsFeatureState};

#[component]
pub fn SettingsScreen() -> impl IntoView {
    let state = SettingsFeatureState::new();

    Effect::new(move |_| {
        state.load();
    });

    Effect::new(move |_| {
        state.form.provider_mode.get();
        state.form.local_model_id.get();
        state.local_models.get();
        state.maybe_preload_selected_local_model();
    });

    let custom_api_selected = state.custom_api_selected();
    let active_api_model_label = state.active_api_model_label();
    let selected_input_device_label =
        Signal::derive(move || match state.form.selected_input_device_id.get() {
            Some(selected_id) => state
                .input_devices
                .get()
                .into_iter()
                .find(|device| device.id == selected_id)
                .map(|device| device.label)
                .unwrap_or_else(|| "Previously selected microphone unavailable".to_string()),
            None => state
                .input_devices
                .get()
                .into_iter()
                .find(|device| device.is_default)
                .map(|device| format!("System default ({})", device.label))
                .unwrap_or_else(|| "System default microphone".to_string()),
        });
    let selected_hotkey_label = Signal::derive(move || {
        let mode_label = match state.form.hotkey_mode.get() {
            HotkeyMode::PushToTalk => "Push-to-talk",
            HotkeyMode::Toggle => "Toggle",
        };
        format!("{mode_label} on {}", state.form.hotkey_shortcut.get())
    });
    let save_configuration = move |_| state.save();

    view! {
        <section class="panel content">
            <SettingsHero />
            <StatusCards
                provider_mode=state.form.provider_mode
                local_model_id=state.form.local_model_id
                api_custom_model_name=state.form.api_custom_model_name
                custom_api_selected=custom_api_selected
                active_api_model_label=active_api_model_label
                selected_input_device_label=selected_input_device_label
                selected_hotkey_label=selected_hotkey_label
            />

            <Show when=move || state.is_loading.get()>
                <LoadingSection />
            </Show>

            <Show when=move || state.load_error.get().is_some()>
                <LoadErrorSection load_error=state.load_error />
            </Show>

            <Show when=move || !state.is_loading.get()>
                <div class="settings-grid">
                    <ProviderSettingsCard
                        state=state
                        custom_api_selected=custom_api_selected
                    />
                    <div class="settings-sidebar">
                        <InputDeviceField
                            selected_input_device_id=state.form.selected_input_device_id
                            input_devices=state.input_devices
                        />
                        <HotkeySettingsCard state=state />
                        <SettingsActionsCard
                            is_saving=state.is_saving
                            save_feedback=state.save_feedback
                            on_save=save_configuration
                        />
                    </div>
                </div>
            </Show>
        </section>
    }
}

#[component]
fn SettingsHero() -> impl IntoView {
    view! {
        <div class="hero">
            <h2>"Provider and model configuration"</h2>
            <p>
                "Choose your transcription provider, save a preferred microphone, and configure the global recording hotkey while keeping API keys out of the plain-text config file."
            </p>
        </div>
    }
}

#[component]
fn StatusCards(
    provider_mode: RwSignal<ProviderMode>,
    local_model_id: RwSignal<String>,
    api_custom_model_name: RwSignal<String>,
    custom_api_selected: Signal<bool>,
    active_api_model_label: Signal<String>,
    selected_input_device_label: Signal<String>,
    selected_hotkey_label: Signal<String>,
) -> impl IntoView {
    view! {
        <div class="status">
            <div class="status-card">
                <p class="status-label">"Selected provider"</p>
                <p class="status-value">
                    {move || match provider_mode.get() {
                        ProviderMode::Local => "Local Whisper".to_string(),
                        ProviderMode::Api => "OpenAI-compatible API".to_string(),
                    }}
                </p>
            </div>
            <div class="status-card">
                <p class="status-label">"Local model"</p>
                <p class="status-value">{move || local_model_id.get()}</p>
            </div>
            <div class="status-card">
                <p class="status-label">"API model"</p>
                <p class="status-value">
                    {move || {
                        if custom_api_selected.get() {
                            let custom_name = api_custom_model_name.get();
                            if custom_name.trim().is_empty() {
                                "Custom model name".to_string()
                            } else {
                                custom_name
                            }
                        } else {
                            active_api_model_label.get()
                        }
                    }}
                </p>
            </div>
            <div class="status-card">
                <p class="status-label">"Microphone"</p>
                <p class="status-value">{move || selected_input_device_label.get()}</p>
            </div>
            <div class="status-card">
                <p class="status-label">"Recording hotkey"</p>
                <p class="status-value">{move || selected_hotkey_label.get()}</p>
            </div>
        </div>
    }
}

#[component]
fn LoadingSection() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Loading"</p>
            <h3>"Fetching saved settings"</h3>
            <p class="body-copy">"Transcribe Kit is loading your provider and model configuration."</p>
        </section>
    }
}

#[component]
fn LoadErrorSection(load_error: RwSignal<Option<String>>) -> impl IntoView {
    view! {
        <section class="section error-section">
            <p class="tag">"Load error"</p>
            <h3>"Some startup data did not load"</h3>
            <p class="body-copy">{move || load_error.get().unwrap_or_default()}</p>
        </section>
    }
}

#[component]
fn ProviderSettingsCard(
    state: SettingsFeatureState,
    custom_api_selected: Signal<bool>,
) -> impl IntoView {
    view! {
        <section class="section settings-card provider-settings-card">
            <p class="tag">"Provider"</p>
            <h3>"Transcription engine"</h3>
            <div class="stack">
                <ProviderModeField provider_mode=state.form.provider_mode />

                <Show when=move || matches!(state.form.provider_mode.get(), ProviderMode::Local)>
                    <LocalModelField
                        local_model_id=state.form.local_model_id
                        local_models=state.local_models
                    />
                    <ModelDownloadSection state=state />
                </Show>

                <Show when=move || matches!(state.form.provider_mode.get(), ProviderMode::Api)>
                    <ApiSettingsFields state=state custom_api_selected=custom_api_selected />
                </Show>
            </div>
        </section>
    }
}

#[component]
fn InputDeviceField(
    selected_input_device_id: RwSignal<Option<String>>,
    input_devices: RwSignal<Vec<AudioInputDeviceDescriptor>>,
) -> impl IntoView {
    let selected_input_device_missing = Signal::derive(move || {
        let Some(selected_id) = selected_input_device_id.get() else {
            return false;
        };

        !input_devices
            .get()
            .into_iter()
            .any(|device| device.id == selected_id)
    });

    let selected_device_hint = Signal::derive(move || {
        let devices = input_devices.get();

        match selected_input_device_id.get() {
            Some(selected_id) => devices
                .into_iter()
                .find(|device| device.id == selected_id)
                .map(|device| describe_input_device(&device))
                .unwrap_or_else(|| {
                    "The saved microphone is no longer available on this machine.".to_string()
                }),
            None => devices
                .into_iter()
                .find(|device| device.is_default)
                .map(|device| format!("System default is currently {}.", device.label))
                .unwrap_or_else(|| {
                    "System default will follow the OS microphone choice when live recording ships in Phase 3b."
                        .to_string()
                }),
        }
    });

    let device_count_label = Signal::derive(move || {
        let count = input_devices.get().len();
        match count {
            0 => "No microphones detected".to_string(),
            1 => "1 microphone detected".to_string(),
            _ => format!("{count} microphones detected"),
        }
    });

    view! {
        <section class="section settings-card input-device-panel">
            <div class="input-device-panel-header">
                <div class="stack">
                    <p class="tag">"Audio input"</p>
                    <h4>"Choose the microphone for live recording"</h4>
                    <p class="body-copy">
                        "This dropdown controls which microphone Phase 3b will use when live capture is enabled."
                    </p>
                </div>
                <span class="mini-chip">{move || device_count_label.get()}</span>
            </div>

            <label class="field">
                <span class="field-label">"Microphone dropdown"</span>
                <select
                    class="input-device-select"
                    prop:value=move || selected_input_device_id.get().unwrap_or_default()
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        selected_input_device_id
                            .set(if value.trim().is_empty() { None } else { Some(value) });
                    }
                >
                    <Show when=move || selected_input_device_missing.get()>
                        <option value=move || selected_input_device_id.get().unwrap_or_default()>
                            "Previously selected microphone (currently unavailable)"
                        </option>
                    </Show>
                    <option value="">"System default microphone"</option>
                    <For
                        each=move || input_devices.get()
                        key=|device| device.id.clone()
                        children=move |device| {
                            let label = if device.is_default {
                                format!("{} (Default)", device.label)
                            } else {
                                device.label.clone()
                            };

                            view! {
                                <option value=device.id.clone()>{label}</option>
                            }
                        }
                    />
                </select>
            </label>

            <p class="field-hint">{move || selected_device_hint.get()}</p>

            <Show when=move || selected_input_device_missing.get()>
                <p class="field-hint field-warning">
                    "Choose a different device or switch to System default before saving."
                </p>
            </Show>

            <Show when=move || input_devices.get().is_empty()>
                <p class="field-hint">
                    "No microphones were detected right now. You can still keep the selection on System default."
                </p>
            </Show>
        </section>
    }
}

#[component]
fn HotkeySettingsCard(state: SettingsFeatureState) -> impl IntoView {
    let is_capturing = RwSignal::new(false);
    let capture_preview = RwSignal::new(None::<String>);
    let capture_feedback = RwSignal::new(None::<String>);

    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };

        let keydown_closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if !is_capturing.get_untracked() || event.repeat() {
                return;
            }

            let code = event.code();
            if code == "Tab" {
                is_capturing.set(false);
                capture_preview.set(None);
                return;
            }

            event.prevent_default();
            event.stop_propagation();

            if code == "Escape" {
                is_capturing.set(false);
                capture_preview.set(None);
                capture_feedback.set(None);
                return;
            }

            if is_modifier_code(&code) {
                capture_preview.set(Some(format_shortcut_preview(&event, None)));
                capture_feedback.set(None);
                return;
            }

            let captured_shortcut = format_shortcut_value(&event);
            capture_preview.set(Some(captured_shortcut.clone()));

            if !shortcut_has_modifier(&event) {
                capture_feedback.set(Some(
                    "Include at least one modifier key like Cmd, Ctrl, Alt, or Shift.".to_string(),
                ));
                return;
            }

            state.form.hotkey_shortcut.set(captured_shortcut);
            capture_feedback.set(None);
            is_capturing.set(false);
        }) as Box<dyn FnMut(web_sys::KeyboardEvent)>);

        let _ = window
            .add_event_listener_with_callback("keydown", keydown_closure.as_ref().unchecked_ref());
        keydown_closure.forget();
    });

    let hotkey_mode_label = Signal::derive(move || match state.form.hotkey_mode.get() {
        HotkeyMode::PushToTalk => {
            "Hold the shortcut to record, then release to stop when live capture is connected."
                .to_string()
        }
        HotkeyMode::Toggle => {
            "Press once to start and again to stop when live capture is connected.".to_string()
        }
    });

    let capture_button_label = Signal::derive(move || {
        if is_capturing.get() {
            capture_preview
                .get()
                .unwrap_or_else(|| "Press the shortcut now".to_string())
        } else {
            state.form.hotkey_shortcut.get()
        }
    });

    view! {
        <section class="section settings-card">
            <p class="tag">"Hotkey"</p>
            <h3>"Global recording shortcut"</h3>
            <p class="body-copy">
                "This shortcut is registered globally, so it can still trigger while Transcribe Kit is running in the background."
            </p>

            <label class="field">
                <span class="field-label">"Recording mode"</span>
                <select
                    prop:value=move || match state.form.hotkey_mode.get() {
                        HotkeyMode::PushToTalk => "push-to-talk",
                        HotkeyMode::Toggle => "toggle",
                    }
                    on:change=move |event| {
                        match event_target_value(&event).as_str() {
                            "toggle" => state.form.hotkey_mode.set(HotkeyMode::Toggle),
                            _ => state.form.hotkey_mode.set(HotkeyMode::PushToTalk),
                        }
                    }
                >
                    <option value="push-to-talk">"Push-to-talk"</option>
                    <option value="toggle">"Toggle"</option>
                </select>
            </label>

            <label class="field">
                <span class="field-label">"Shortcut"</span>
                <button
                    type="button"
                    class="hotkey-capture-button"
                    class:hotkey-capture-button-listening=move || is_capturing.get()
                    on:click=move |_| {
                        let next = !is_capturing.get_untracked();
                        is_capturing.set(next);
                        capture_preview.set(None);
                        capture_feedback.set(None);
                    }
                >
                    {move || capture_button_label.get()}
                </button>
            </label>

            <p class="field-hint">
                "Click the control, then press the shortcut you want. Press Escape to cancel or Tab to move on."
            </p>
            <p class="field-hint">{move || hotkey_mode_label.get()}</p>
            <p class="field-hint">
                "Test it by pressing the hotkey here for an in-app banner, then switch to another app and press it again to flash the dock or taskbar."
            </p>

            <Show when=move || capture_feedback.get().is_some()>
                <p class="field-hint field-warning">
                    {move || capture_feedback.get().unwrap_or_default()}
                </p>
            </Show>

            <Show when=move || state.hotkey_registration_error.get().is_some()>
                <p class="field-hint field-warning">
                    {move || state.hotkey_registration_error.get().unwrap_or_default()}
                </p>
            </Show>
        </section>
    }
}

fn shortcut_has_modifier(event: &web_sys::KeyboardEvent) -> bool {
    event.ctrl_key() || event.alt_key() || event.shift_key() || event.meta_key()
}

fn is_modifier_code(code: &str) -> bool {
    matches!(
        code,
        "AltLeft"
            | "AltRight"
            | "ControlLeft"
            | "ControlRight"
            | "MetaLeft"
            | "MetaRight"
            | "ShiftLeft"
            | "ShiftRight"
    )
}

fn format_shortcut_value(event: &web_sys::KeyboardEvent) -> String {
    format_shortcut_preview(event, Some(event.code()))
}

fn format_shortcut_preview(event: &web_sys::KeyboardEvent, key_code: Option<String>) -> String {
    let mut parts = Vec::new();

    if event.ctrl_key() {
        parts.push("Ctrl".to_string());
    }
    if event.alt_key() {
        parts.push("Alt".to_string());
    }
    if event.shift_key() {
        parts.push("Shift".to_string());
    }
    if event.meta_key() {
        parts.push("Cmd".to_string());
    }
    if let Some(key_code) = key_code {
        parts.push(key_code);
    }

    parts.join("+")
}

#[component]
fn SettingsActionsCard(
    is_saving: RwSignal<bool>,
    save_feedback: RwSignal<Option<String>>,
    on_save: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
) -> impl IntoView {
    view! {
        <section class="section settings-card settings-actions-card">
            <p class="tag">"Apply"</p>
            <h3>"Save configuration"</h3>
            <p class="body-copy">
                "Provider, model, microphone, and hotkey preferences are stored together so the app opens in the same state next time."
            </p>

            <button class="primary-button" on:click=on_save disabled=move || is_saving.get()>
                {move || if is_saving.get() { "Saving..." } else { "Save settings" }}
            </button>

            <Show when=move || save_feedback.get().is_some()>
                <p class="feedback">{move || save_feedback.get().unwrap_or_default()}</p>
            </Show>
        </section>
    }
}

#[component]
fn ProviderModeField(provider_mode: RwSignal<ProviderMode>) -> impl IntoView {
    view! {
        <label class="field">
            <span class="field-label">"Mode"</span>
            <select
                prop:value=move || match provider_mode.get() {
                    ProviderMode::Local => "local",
                    ProviderMode::Api => "api",
                }
                on:change=move |event| {
                    match event_target_value(&event).as_str() {
                        "api" => provider_mode.set(ProviderMode::Api),
                        _ => provider_mode.set(ProviderMode::Local),
                    }
                }
            >
                <option value="local">"Local Whisper"</option>
                <option value="api">"OpenAI-compatible API"</option>
            </select>
        </label>
    }
}

#[component]
fn LocalModelField(
    local_model_id: RwSignal<String>,
    local_models: RwSignal<Vec<crate::tauri_api::LocalModelDescriptor>>,
) -> impl IntoView {
    view! {
        <label class="field">
            <span class="field-label">"Whisper model"</span>
            <select
                prop:value=move || local_model_id.get()
                on:change=move |event| local_model_id.set(event_target_value(&event))
            >
                <For
                    each=move || local_models.get()
                    key=|model| model.id.clone()
                    children=move |model| {
                        view! {
                            <option value=model.id.clone()>{model.label}</option>
                        }
                    }
                />
            </select>
        </label>
    }
}

#[component]
fn ModelDownloadSection(state: SettingsFeatureState) -> impl IntoView {
    let model_downloaded = state.selected_model_downloaded();
    let download = state.download;
    let on_download = move |_| state.download_selected_model();
    let on_delete = move |_| state.delete_selected_model();

    let selected_size_label = Signal::derive(move || {
        let model_id = state.form.local_model_id.get();
        state
            .local_models
            .get()
            .iter()
            .find(|m| m.id == model_id)
            .map(|m| m.size_label.clone())
            .unwrap_or_default()
    });

    view! {
        <div class="model-download-section">
            <Show
                when=move || download.is_downloading.get()
                fallback=move || {
                    view! {
                        <Show
                            when=move || model_downloaded.get()
                            fallback=move || {
                                view! {
                                    <div class="model-status not-downloaded">
                                        <span class="status-dot missing"></span>
                                        <span>"Model not downloaded"</span>
                                        <span class="size-hint">{move || selected_size_label.get()}</span>
                                        <button
                                            class="download-button"
                                            on:click=on_download
                                        >
                                            "Download"
                                        </button>
                                    </div>
                                }
                            }
                        >
                            <div class="model-status downloaded">
                                <span class="status-dot ready"></span>
                                <span>"Model ready"</span>
                                <button
                                    class="delete-button"
                                    on:click=on_delete
                                >
                                    "Delete"
                                </button>
                            </div>
                        </Show>
                    }
                }
            >
                <DownloadProgressBar download=download />
            </Show>

            <Show when=move || download.download_error.get().is_some()>
                <p class="download-error">{move || download.download_error.get().unwrap_or_default()}</p>
            </Show>
        </div>
    }
}

#[component]
fn DownloadProgressBar(download: DownloadState) -> impl IntoView {
    let fraction = download.progress_fraction();

    let percent_label = Signal::derive(move || {
        let pct = (fraction.get() * 100.0).round() as u32;
        format!("{pct}%")
    });

    let downloaded_label = Signal::derive(move || {
        let bytes = download.downloaded_bytes.get();
        format_bytes(bytes)
    });

    let total_label = Signal::derive(move || {
        download
            .total_bytes
            .get()
            .map(format_bytes)
            .unwrap_or_default()
    });

    view! {
        <div class="download-progress">
            <div class="progress-header">
                <span>"Downloading model..."</span>
                <span class="progress-stats">
                    {move || downloaded_label.get()}
                    {move || {
                        let total = total_label.get();
                        if total.is_empty() { String::new() } else { format!(" / {total}") }
                    }}
                </span>
            </div>
            <div class="progress-bar-track">
                <div
                    class="progress-bar-fill"
                    style:width=move || percent_label.get()
                ></div>
            </div>
            <span class="progress-percent">{move || percent_label.get()}</span>
        </div>
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.0} KB");
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{mb:.1} MB");
    }
    let gb = mb / 1024.0;
    format!("{gb:.2} GB")
}

fn describe_input_device(device: &AudioInputDeviceDescriptor) -> String {
    let mut details = Vec::new();

    if let Some(manufacturer) = device.manufacturer.as_deref() {
        details.push(manufacturer.to_string());
    }

    if let Some(channels) = device.channels {
        details.push(format!("{channels} ch"));
    }

    if let Some(sample_rate_hz) = device.sample_rate_hz {
        details.push(format!("{sample_rate_hz} Hz"));
    }

    if details.is_empty() {
        format!("{} is ready for live recording.", device.label)
    } else {
        format!("{}: {}", device.label, details.join(" • "))
    }
}

#[component]
fn ApiSettingsFields(
    state: SettingsFeatureState,
    custom_api_selected: Signal<bool>,
) -> impl IntoView {
    view! {
        <div class="stack">
            <label class="field">
                <span class="field-label">"API model"</span>
                <select
                    prop:value=move || state.form.api_model_id.get()
                    on:change=move |event| state.form.api_model_id.set(event_target_value(&event))
                >
                    <For
                        each=move || state.api_models.get()
                        key=|model| model.id.clone()
                        children=move |model| {
                            view! {
                                <option value=model.id.clone()>{model.label}</option>
                            }
                        }
                    />
                </select>
            </label>

            <Show when=move || custom_api_selected.get()>
                <label class="field">
                    <span class="field-label">"Custom model name"</span>
                    <input
                        type="text"
                        prop:value=move || state.form.api_custom_model_name.get()
                        on:input=move |event| state
                            .form
                            .api_custom_model_name
                            .set(event_target_value(&event))
                        placeholder="Enter any OpenAI-compatible model string"
                    />
                </label>
            </Show>

            <label class="field">
                <span class="field-label">"Base URL"</span>
                <input
                    type="url"
                    prop:value=move || state.form.api_base_url.get()
                    on:input=move |event| state.form.api_base_url.set(event_target_value(&event))
                    placeholder="https://api.openai.com/v1"
                />
            </label>

            <label class="field">
                <span class="field-label">"API key"</span>
                <input
                    type="password"
                    prop:value=move || state.form.api_key_input.get()
                    on:input=move |event| {
                        state.form.api_key_input.set(event_target_value(&event));
                        state.form.clear_api_key.set(false);
                    }
                    placeholder=move || {
                        if state.form.api_key_present.get() {
                            "Leave blank to keep the saved key"
                        } else {
                            "Stored in the system credential store"
                        }
                    }
                />
            </label>

            <label class="checkbox-row">
                <input
                    type="checkbox"
                    prop:checked=move || state.form.clear_api_key.get()
                    on:change=move |event| state.form.clear_api_key.set(event_target_checked(&event))
                />
                <span>"Remove the saved API key for this base URL"</span>
            </label>
        </div>
    }
}
