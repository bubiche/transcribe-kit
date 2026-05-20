use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::live_recording::LiveRecordingController;
use crate::tauri_api::{HotkeyMode, ProviderMode};

use super::components::{
    ApiConnectionCard, AutoSaveIndicator, CaptureProfileField, DangerZoneCard, HotkeySettingsCard,
    InputDeviceField, PostprocessSettingsCard, ProviderSettingsCard,
};
use super::state::{AutoSaveStatus, SettingsFeatureState};

#[component]
pub fn SettingsScreen(
    live_recording: LiveRecordingController,
    show_settings: RwSignal<bool>,
) -> impl IntoView {
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

    // Debounced auto-save: subscribe to all form signals except api_key_input
    let controller = live_recording;
    Effect::new(move |_| {
        let _ = state.form.provider_mode.get();
        let _ = state.form.local_model_id.get();
        let _ = state.form.selected_input_device_id.get();
        let _ = state.form.live_capture_profile.get();
        let _ = state.form.hotkey_mode.get();
        let _ = state.form.hotkey_shortcut.get();
        let _ = state.form.api_model_id.get();
        let _ = state.form.api_custom_model_name.get();
        let _ = state.form.api_base_url.get();
        let _ = state.form.clear_api_key.get();
        let _ = state.form.postprocess_model.get();
        let _ = state.form.postprocess_provider_mode.get();
        let _ = state.form.local_llm_model_id.get();

        if state.suppress_auto_save.get_untracked()
            || state.is_loading.get_untracked()
            || state.is_saving.get_untracked()
        {
            return;
        }

        let next_gen = state.auto_save_generation.get_untracked().saturating_add(1);
        state.auto_save_generation.set(next_gen);
        state.auto_save_status.set(AutoSaveStatus::Pending);

        let timeout_closure = Closure::once_into_js(move || {
            if state.auto_save_generation.get_untracked() != next_gen {
                return;
            }
            if state.suppress_auto_save.get_untracked() || state.is_saving.get_untracked() {
                return;
            }
            state.save(move || controller.refresh_armed_device_context());
        });

        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_closure.as_ref().unchecked_ref(),
                800,
            );
        }
    });

    // Immediate save on API key blur
    Effect::new(move |_| {
        let gen = state.api_key_save_requested.get();
        if gen == 0 || state.suppress_auto_save.get_untracked() || state.is_loading.get_untracked()
        {
            return;
        }
        state.save(move || controller.refresh_armed_device_context());
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
                .unwrap_or_else(|| "Previously selected input unavailable".to_string()),
            None => state
                .input_devices
                .get()
                .into_iter()
                .find(|device| device.is_default)
                .map(|device| format!("System default ({})", device.label))
                .unwrap_or_else(|| "System default input".to_string()),
        });
    let selected_hotkey_label = Signal::derive(move || {
        let mode_label = match state.form.hotkey_mode.get() {
            HotkeyMode::PushToTalk => "Push-to-talk",
            HotkeyMode::Toggle => "Toggle",
        };
        format!("{mode_label} on {}", state.form.hotkey_shortcut.get())
    });

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
                    <div class="settings-main">
                        <ProviderSettingsCard
                            state=state
                            custom_api_selected=custom_api_selected
                        />
                        <CaptureProfileField
                            live_capture_profile=state.form.live_capture_profile
                        />
                        <InputDeviceField
                            selected_input_device_id=state.form.selected_input_device_id
                            input_devices=state.input_devices
                            live_capture_profile=state.form.live_capture_profile
                            settings_visible=Signal::derive(move || show_settings.get())
                        />
                        <ApiConnectionCard state=state />
                        <PostprocessSettingsCard state=state />
                        <DangerZoneCard state=state />
                    </div>
                    <div class="settings-sidebar">
                        <HotkeySettingsCard state=state />
                        <AutoSaveIndicator state=state />
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
            <h2>"Settings"</h2>
            <p>
                "Configure transcription, API connection, audio capture, and recording hotkey."
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
                <p class="status-label">"Audio input"</p>
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
