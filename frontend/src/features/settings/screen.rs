use leptos::prelude::*;

use crate::{
    features::{audio::AudioFeatureCard, postprocess::PostProcessFeatureCard},
    tauri_api::ProviderMode,
};

use super::state::{DownloadState, SettingsFeatureState};

#[component]
pub fn SettingsScreen() -> impl IntoView {
    let state = SettingsFeatureState::new();

    Effect::new(move |_| {
        state.load();
    });

    let custom_api_selected = state.custom_api_selected();
    let active_api_model_label = state.active_api_model_label();
    let save_configuration = move |_| state.save();

    view! {
        <div class="frame">
            <SettingsSidebar />

            <section class="panel content">
                <SettingsHero />
                <StatusCards
                    provider_mode=state.form.provider_mode
                    local_model_id=state.form.local_model_id
                    api_custom_model_name=state.form.api_custom_model_name
                    custom_api_selected=custom_api_selected
                    active_api_model_label=active_api_model_label
                />

                <Show when=move || state.is_loading.get()>
                    <LoadingSection />
                </Show>

                <Show when=move || state.load_error.get().is_some()>
                    <LoadErrorSection load_error=state.load_error />
                </Show>

                <Show when=move || !state.is_loading.get()>
                    <div class="workspace-grid">
                        <ProviderSettingsForm
                            state=state
                            custom_api_selected=custom_api_selected
                            on_save=save_configuration
                        />
                        <PhaseNotesCard />
                        <RoadmapCard
                            api_key_present=state.form.api_key_present
                            custom_api_selected=custom_api_selected
                        />
                        <AudioFeatureCard />
                        <PostProcessFeatureCard />
                    </div>
                </Show>
            </section>
        </div>
    }
}

#[component]
fn SettingsSidebar() -> impl IntoView {
    view! {
        <aside class="panel sidebar">
            <p class="tag">"Phase 1"</p>
            <h1 class="brand">"Transcribe Kit"</h1>
            <p class="lede">
                "Provider settings now persist locally, and API keys can live in the OS credential store."
            </p>

            <div class="nav">
                <div class="nav-chip">"Local Whisper model selection"</div>
                <div class="nav-chip">"OpenAI-compatible API settings"</div>
                <div class="nav-chip">"Saved config + keychain secrets"</div>
                <div class="nav-chip">"Ready for file and mic flows next"</div>
            </div>
        </aside>
    }
}

#[component]
fn SettingsHero() -> impl IntoView {
    view! {
        <div class="hero">
            <h2>"Provider and model configuration"</h2>
            <p>
                "Choose between local Whisper and API transcription, save your preferred model, and keep the API key out of the plain-text config file."
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
fn ProviderSettingsForm(
    state: SettingsFeatureState,
    custom_api_selected: Signal<bool>,
    on_save: impl Fn(leptos::ev::MouseEvent) + Copy + 'static,
) -> impl IntoView {
    view! {
        <section class="section form-section">
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

                <button class="primary-button" on:click=on_save disabled=move || state.is_saving.get()>
                    {move || if state.is_saving.get() { "Saving..." } else { "Save settings" }}
                </button>

                <Show when=move || state.save_feedback.get().is_some()>
                    <p class="feedback">{move || state.save_feedback.get().unwrap_or_default()}</p>
                </Show>
            </div>
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

#[component]
fn PhaseNotesCard() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Notes"</p>
            <h3>"Phase 1 coverage"</h3>
            <ul class="list">
                <li>"Settings are saved to a local config file."</li>
                <li>"API keys stay in the OS keychain, not in plain text."</li>
                <li>"Whisper choices are Tiny, Base, Small, and Large v3 Turbo."</li>
                <li>"API choices are GPT-4o mini Transcribe, GPT-4o Transcribe, or a custom model string."</li>
            </ul>
        </section>
    }
}

#[component]
fn RoadmapCard(
    api_key_present: RwSignal<bool>,
    custom_api_selected: Signal<bool>,
) -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Roadmap"</p>
            <h3>"What this unlocks next"</h3>
            <p class="body-copy">
                "The app now has a durable provider configuration layer, which gives phase 2 and phase 4 a stable place to read model and credential choices from."
            </p>
            <div class="mini-status">
                <span class="mini-chip">{move || if api_key_present.get() { "API key saved" } else { "No API key saved" }}</span>
                <span class="mini-chip">
                    {move || if custom_api_selected.get() { "Custom API model ready" } else { "Preset API model selected" }}
                </span>
            </div>
        </section>
    }
}
