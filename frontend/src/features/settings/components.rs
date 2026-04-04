use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::tauri_api::{AudioInputDeviceDescriptor, HotkeyMode, LiveCaptureProfile, ProviderMode};

use super::input_device_hints::{
    classify_input_device, describe_input_device, format_input_device_option_label,
    input_device_kind_summary, system_default_option_label, InputDeviceKindHint,
};
use super::meeting_capture::{
    build_capture_behavior_summary, build_meeting_readiness_hint,
    build_meeting_troubleshooting_steps, detect_runtime_platform, platform_meeting_guidance,
};
use super::state::{DownloadState, SettingsFeatureState};

#[component]
pub(super) fn ProviderSettingsCard(
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
pub(super) fn CaptureProfileField(
    live_capture_profile: RwSignal<LiveCaptureProfile>,
) -> impl IntoView {
    let profile_hint = Signal::derive(move || {
        match live_capture_profile.get() {
        LiveCaptureProfile::MicrophoneOnly => {
            "Records from your selected audio input. Best for voice notes and dictation."
                .to_string()
        }
        LiveCaptureProfile::MeetingMix => {
            "Records from an input that already contains both your voice and remote call audio, such as a loopback, monitor, or virtual cable input."
                .to_string()
        }
    }
    });

    view! {
        <section class="section settings-card">
            <p class="tag">"Capture"</p>
            <h4>"Capture profile"</h4>
            <p class="body-copy">
                "Choose whether you are recording just your own voice or capturing a full meeting."
            </p>

            <label class="field">
                <span class="field-label">"Profile"</span>
                <select
                    prop:value=move || match live_capture_profile.get() {
                        LiveCaptureProfile::MicrophoneOnly => "microphone-only",
                        LiveCaptureProfile::MeetingMix => "meeting-mix",
                    }
                    on:change=move |event| {
                        match event_target_value(&event).as_str() {
                            "meeting-mix" => live_capture_profile.set(LiveCaptureProfile::MeetingMix),
                            _ => live_capture_profile.set(LiveCaptureProfile::MicrophoneOnly),
                        }
                    }
                >
                    <option value="microphone-only">"Microphone only"</option>
                    <option value="meeting-mix">"Meeting mix"</option>
                </select>
            </label>

            <p class="field-hint">{move || profile_hint.get()}</p>
        </section>
    }
}

#[component]
pub(super) fn InputDeviceField(
    selected_input_device_id: RwSignal<Option<String>>,
    input_devices: RwSignal<Vec<AudioInputDeviceDescriptor>>,
    live_capture_profile: RwSignal<LiveCaptureProfile>,
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
        let selected_id = selected_input_device_id.get();

        match selected_id.as_deref() {
            Some(selected_id) => devices
                .into_iter()
                .find(|device| device.id == selected_id)
                .map(|device| describe_input_device(&device))
                .unwrap_or_else(|| {
                    "The saved audio input is no longer available on this machine.".to_string()
                }),
            None => devices
                .into_iter()
                .find(|device| device.is_default)
                .map(|device| {
                    format!(
                        "System default is currently {}. {}",
                        device.label.clone(),
                        input_device_kind_summary(classify_input_device(&device))
                    )
                })
                .unwrap_or_else(|| {
                    "System default will follow the OS audio input choice whenever live capture starts."
                        .to_string()
                }),
        }
    });

    let meeting_mix_mic_warning = Signal::derive(move || {
        if !matches!(live_capture_profile.get(), LiveCaptureProfile::MeetingMix) {
            return false;
        }
        let devices = input_devices.get();
        let effective_device = match selected_input_device_id.get() {
            Some(selected_id) => devices.iter().find(|d| d.id == selected_id),
            None => devices.iter().find(|d| d.is_default),
        };
        effective_device
            .map(|device| classify_input_device(device) == InputDeviceKindHint::PhysicalMic)
            .unwrap_or(false)
    });

    let device_count_label = Signal::derive(move || {
        let count = input_devices.get().len();
        match count {
            0 => "No audio inputs detected".to_string(),
            1 => "1 audio input detected".to_string(),
            _ => format!("{count} audio inputs detected"),
        }
    });

    let meeting_readiness_hint = Signal::derive(move || {
        let devices = input_devices.get();
        let selected_id = selected_input_device_id.get();
        build_meeting_readiness_hint(live_capture_profile.get(), selected_id.as_deref(), &devices)
    });

    view! {
        <section class="section settings-card input-device-panel">
            <div class="input-device-panel-header">
                <div class="stack">
                    <p class="tag">"Audio input"</p>
                    <h4>"Choose the audio input for live capture"</h4>
                    <p class="body-copy">
                        "This dropdown controls which audio input the live capture pipeline will use when recording starts."
                    </p>
                </div>
                <span class="mini-chip">{move || device_count_label.get()}</span>
            </div>

            <label class="field">
                <span class="field-label">"Audio input"</span>
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
                            "Previously selected input (currently unavailable)"
                        </option>
                    </Show>
                    <option value="">
                        {move || system_default_option_label(&input_devices.get())}
                    </option>
                    <For
                        each=move || input_devices.get()
                        key=|device| device.id.clone()
                        children=move |device| {
                            view! {
                                <option value=device.id.clone()>{format_input_device_option_label(&device)}</option>
                            }
                        }
                    />
                </select>
            </label>

            <p class="field-hint">{move || selected_device_hint.get()}</p>

            <div class=move || format!(
                "meeting-readiness meeting-readiness-{}",
                meeting_readiness_hint.get().tone.class_name()
            )>
                <div class="meeting-readiness-header">
                    <p class="field-label">"Will this work for meetings?"</p>
                    <Show when=move || meeting_readiness_hint.get().device_kind.is_some()>
                        <span class=move || {
                            let hint = meeting_readiness_hint.get();
                            let kind = hint.device_kind.unwrap_or(InputDeviceKindHint::Unknown);
                            format!(
                                "mini-chip device-kind-chip device-kind-chip-{}",
                                kind.class_name()
                            )
                        }>
                            {move || {
                                let hint = meeting_readiness_hint.get();
                                hint.device_kind
                                    .map(|kind| kind.badge_label().to_string())
                                    .unwrap_or_default()
                            }}
                        </span>
                    </Show>
                </div>
                <p class="meeting-readiness-title">{move || meeting_readiness_hint.get().title}</p>
                <p class="field-hint">{move || meeting_readiness_hint.get().body}</p>
            </div>

            <Show when=move || meeting_mix_mic_warning.get()>
                <p class="field-hint field-warning">
                    "Meeting capture may be incomplete: the selected input looks like a standard microphone rather than a mixed input. The transcript may mostly contain your own voice."
                </p>
            </Show>

            <Show when=move || selected_input_device_missing.get()>
                <p class="field-hint field-warning">
                    "Choose a different device or switch to System default before saving."
                </p>
            </Show>

            <Show when=move || input_devices.get().is_empty()>
                <p class="field-hint">
                    "No audio inputs were detected right now. You can still keep the selection on System default."
                </p>
            </Show>

            <MeetingCaptureSetupHelp
                selected_input_device_id=selected_input_device_id
                input_devices=input_devices
                live_capture_profile=live_capture_profile
            />
        </section>
    }
}

#[component]
fn MeetingCaptureSetupHelp(
    selected_input_device_id: RwSignal<Option<String>>,
    input_devices: RwSignal<Vec<AudioInputDeviceDescriptor>>,
    live_capture_profile: RwSignal<LiveCaptureProfile>,
) -> impl IntoView {
    let platform = detect_runtime_platform();
    let behavior_summary = Signal::derive(move || {
        let devices = input_devices.get();
        let selected_id = selected_input_device_id.get();
        build_capture_behavior_summary(live_capture_profile.get(), selected_id.as_deref(), &devices)
    });
    let troubleshooting_steps = Signal::derive(move || {
        let devices = input_devices.get();
        let selected_id = selected_input_device_id.get();
        build_meeting_troubleshooting_steps(
            platform,
            live_capture_profile.get(),
            selected_id.as_deref(),
            &devices,
        )
    });
    let platform_guidance = platform_meeting_guidance(platform);

    view! {
        <div class="meeting-setup-stack">
            <div class=move || format!(
                "capture-behavior capture-behavior-{}",
                behavior_summary.get().tone.class_name()
            )>
                <p class="field-label">"Before you record"</p>
                <p class="meeting-readiness-title">{move || behavior_summary.get().title}</p>
                <p class="field-hint">{move || behavior_summary.get().body}</p>
            </div>

            <Show when=move || matches!(live_capture_profile.get(), LiveCaptureProfile::MeetingMix)>
                <div class="meeting-guide-card meeting-guide-card-accent">
                    <div class="meeting-guide-header">
                        <p class="field-label">"First time using Meeting mix?"</p>
                        <span class="mini-chip">"Meeting mix"</span>
                    </div>
                    <p class="meeting-readiness-title">
                        "Choose a mixed input before you start recording."
                    </p>
                    <p class="field-hint">
                        "For full meeting capture, choose an audio input that already contains both your microphone and the call audio, such as a loopback, monitor, or virtual cable input. Transcribe Kit records whichever input source you select."
                    </p>
                </div>

                <div class="meeting-guide-card">
                    <div class="meeting-guide-header">
                        <p class="field-label">"Platform guidance"</p>
                        <span class="mini-chip">
                            {format!("Current platform: {}", platform_guidance.platform_label)}
                        </span>
                    </div>
                    <p class="meeting-readiness-title">{platform_guidance.title}</p>
                    <p class="field-hint">{platform_guidance.body}</p>
                </div>

                <div class="meeting-guide-card">
                    <p class="field-label">"Troubleshooting"</p>
                    <ul class="meeting-help-list">
                        <For
                            each=move || troubleshooting_steps.get()
                            key=|step| step.clone()
                            children=move |step| view! { <li>{step}</li> }
                        />
                    </ul>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub(super) fn HotkeySettingsCard(state: SettingsFeatureState) -> impl IntoView {
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
pub(super) fn SettingsActionsCard(
    is_saving: RwSignal<bool>,
    save_feedback: RwSignal<Option<String>>,
    on_save: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
) -> impl IntoView {
    view! {
        <section class="section settings-card settings-actions-card">
            <p class="tag">"Apply"</p>
            <h3>"Save configuration"</h3>
            <p class="body-copy">
                "Provider, model, capture profile, audio input, and hotkey preferences are stored together so the app opens in the same state next time."
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

#[component]
pub(super) fn ApiSettingsFields(
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
