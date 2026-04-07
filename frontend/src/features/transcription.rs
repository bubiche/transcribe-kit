mod controller;
mod panels;
mod utils;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::live_recording::LiveRecordingController;
use crate::tauri_api::{
    get_settings, list_api_models, list_local_models, pick_audio_file, start_file_transcription,
    ApiModelDescriptor, AppSettings, InputType, LiveRecordingState, LocalModelDescriptor,
    ProviderMode,
};

pub use self::controller::TranscriptionController;
pub use self::panels::{JobStatusPanel, TranscriptResultPanel};
use self::utils::file_name_from_path;
pub use self::utils::format_timestamp;

#[component]
pub fn TranscribeScreen(
    active: Signal<bool>,
    transcription: TranscriptionController,
    live_recording: LiveRecordingController,
    live_recording_state: Signal<LiveRecordingState>,
    live_recording_label: Signal<String>,
    live_recording_elapsed_ms: Signal<u64>,
) -> impl IntoView {
    let settings = RwSignal::new(AppSettings::default());
    let local_models = RwSignal::new(Vec::<LocalModelDescriptor>::new());
    let api_models = RwSignal::new(Vec::<ApiModelDescriptor>::new());
    let selected_file = RwSignal::new(None::<String>);
    let load_error = RwSignal::new(None::<String>);
    let is_loading = RwSignal::new(true);

    Effect::new(move |_| {
        if !active.get() {
            return;
        }

        spawn_local(async move {
            is_loading.set(true);
            load_error.set(None);

            let settings_result = get_settings().await;
            let models_result = list_local_models().await;
            let api_models_result = list_api_models().await;

            match (settings_result, models_result, api_models_result) {
                (Ok(loaded_settings), Ok(models), Ok(api)) => {
                    settings.set(loaded_settings);
                    local_models.set(models);
                    api_models.set(api);
                }
                (settings_result, models_result, api_models_result) => {
                    let mut problems = Vec::new();

                    if let Err(error) = settings_result {
                        problems.push(format!("settings: {error}"));
                    }
                    if let Err(error) = models_result {
                        problems.push(format!("local models: {error}"));
                    }
                    if let Err(error) = api_models_result {
                        problems.push(format!("api models: {error}"));
                    }

                    load_error.set(Some(problems.join(" | ")));
                }
            }

            is_loading.set(false);
        });
    });

    let selected_model = Signal::derive(move || {
        let model_id = settings.get().local_model_id;
        local_models
            .get()
            .into_iter()
            .find(|model| model.id == model_id)
    });

    let model_ready = Signal::derive(move || {
        selected_model
            .get()
            .map(|model| model.downloaded)
            .unwrap_or(false)
    });

    let model_label = Signal::derive(move || match settings.get().provider_mode {
        ProviderMode::Local => selected_model
            .get()
            .map(|m| m.label)
            .unwrap_or_else(|| settings.get().local_model_id),
        ProviderMode::Api => {
            let s = settings.get();
            if s.api_model_id == "custom" {
                let name = s.api_custom_model_name.trim().to_string();
                if name.is_empty() {
                    "Custom model".to_string()
                } else {
                    name
                }
            } else {
                api_models
                    .get()
                    .into_iter()
                    .find(|m| m.id == s.api_model_id)
                    .map(|m| m.label)
                    .unwrap_or(s.api_model_id)
            }
        }
    });

    let provider_ready = Signal::derive(move || match settings.get().provider_mode {
        ProviderMode::Local => model_ready.get(),
        ProviderMode::Api => settings.get().api_key_present,
    });

    let provider_label = Signal::derive(move || match settings.get().provider_mode {
        ProviderMode::Local => "Local Whisper".to_string(),
        ProviderMode::Api => "OpenAI-compatible API".to_string(),
    });

    let selected_file_label = Signal::derive(move || {
        selected_file
            .get()
            .as_deref()
            .map(file_name_from_path)
            .unwrap_or_else(|| "No file selected yet".to_string())
    });

    let is_listening =
        Signal::derive(move || matches!(live_recording_state.get(), LiveRecordingState::Recording));

    let file_button_label = Signal::derive(move || {
        if transcription.is_transcribing.get() {
            "Transcribing...".to_string()
        } else {
            "Choose audio file".to_string()
        }
    });

    let live_button_label = Signal::derive(move || {
        if is_listening.get() {
            "Stop recording".to_string()
        } else {
            "Start live capture".to_string()
        }
    });

    let live_button_disabled = Signal::derive(move || {
        transcription.is_transcribing.get() || !live_recording.is_ready.get()
    });

    let on_toggle_live = move |_| {
        live_recording.toggle_recording();
    };

    let on_choose_file = move |_| {
        spawn_local(async move {
            if is_listening.get_untracked() || transcription.is_transcribing.get_untracked() {
                return;
            }

            transcription.reset_job_feedback();

            match settings.get_untracked().provider_mode {
                ProviderMode::Local => {
                    if !model_ready.get_untracked() {
                        transcription.set_preflight_failure(
                            InputType::File,
                            "The selected Whisper model is not downloaded yet. Download it from Settings and try again.",
                        );
                        return;
                    }
                }
                ProviderMode::Api => {
                    if !settings.get_untracked().api_key_present {
                        transcription.set_preflight_failure(
                            InputType::File,
                            "No API key configured. Add your API key in Settings before importing a file.",
                        );
                        return;
                    }
                }
            }

            let Some(file_path) = (match pick_audio_file().await {
                Ok(path) => path,
                Err(error) => {
                    transcription.set_preflight_failure(InputType::File, error);
                    return;
                }
            }) else {
                return;
            };

            let file_name = file_name_from_path(&file_path);
            selected_file.set(Some(file_path.clone()));
            transcription.start_file_job(file_name.clone());

            let progress_controller = transcription;

            match start_file_transcription(&file_path, move |event| {
                progress_controller.apply_stream_event(event);
            })
            .await
            {
                Ok(result) => {
                    transcription.complete_job(result);
                }
                Err(error) => {
                    transcription.fail_job(InputType::File, Some(file_name), error);
                }
            }
        });
    };

    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Transcription"</h2>
                <p>
                    "Start a live capture or import an audio file to generate a transcript using the selected provider."
                </p>
            </div>

            <div class="status">
                <div class="status-card">
                    <p class="status-label">"Provider"</p>
                    <p class="status-value">{move || provider_label.get()}</p>
                </div>
                <div class="status-card">
                    <p class="status-label">
                        {move || match settings.get().provider_mode {
                            ProviderMode::Local => "Whisper model",
                            ProviderMode::Api => "API model",
                        }}
                    </p>
                    <p class="status-value">{move || model_label.get()}</p>
                </div>
                <div class="status-card">
                    <p class="status-label">
                        {move || match settings.get().provider_mode {
                            ProviderMode::Local => "Model status",
                            ProviderMode::Api => "API status",
                        }}
                    </p>
                    <p class="status-value">
                        {move || if provider_ready.get() { "Ready" } else {
                            match settings.get().provider_mode {
                                ProviderMode::Local => "Download required",
                                ProviderMode::Api => "API key required",
                            }
                        }}
                    </p>
                </div>
                <div class="status-card">
                    <p class="status-label">"Selected file"</p>
                    <p class="status-value">{move || selected_file_label.get()}</p>
                </div>
            </div>

            <Show when=move || is_loading.get()>
                <section class="section">
                    <p class="tag">"Loading"</p>
                    <h3>"Preparing the transcription workspace"</h3>
                    <p class="body-copy">"Fetching saved settings and model metadata."</p>
                </section>
            </Show>

            <Show when=move || load_error.get().is_some()>
                <section class="section error-section">
                    <p class="tag">"Load error"</p>
                    <h3>"Transcription controls are not ready yet"</h3>
                    <p class="body-copy">{move || load_error.get().unwrap_or_default()}</p>
                </section>
            </Show>

            <Show when=move || !is_loading.get() && load_error.get().is_none()>
                <div class="workspace-grid transcription-grid">
                    <section class="section import-panel">
                        <div class="import-layout">
                            <div class="import-copy">
                                <p class="tag">"Capture"</p>
                                <h3>"Live capture or file import"</h3>
                                <p class="body-copy">
                                    {move || match settings.get().provider_mode {
                                        ProviderMode::Local => "Start a live recording from the selected audio input, or import a local audio file. Supported import formats: WAV, MP3, FLAC, OGG, and M4A.",
                                        ProviderMode::Api => "Start a live recording from the selected audio input, or import a local audio file. Supported import formats: WAV, MP3, M4A, MP4, and WebM.",
                                    }}
                                </p>
                            </div>

                            <div class="import-actions">
                                <div class="capture-buttons">
                                    <button
                                        class="primary-button"
                                        class:primary-button-active=move || is_listening.get()
                                        on:click=on_toggle_live
                                        disabled=move || live_button_disabled.get()
                                    >
                                        {move || live_button_label.get()}
                                    </button>
                                    <button
                                        class="secondary-button"
                                        on:click=on_choose_file
                                        disabled=move || {
                                            is_listening.get() || transcription.is_transcribing.get()
                                        }
                                    >
                                        {move || file_button_label.get()}
                                    </button>
                                </div>

                                <div class="mini-status">
                                    <span class="mini-chip">
                                        {move || format!("Provider: {}", provider_label.get())}
                                    </span>
                                    <span class="mini-chip">
                                        {move || format!("Model: {}", model_label.get())}
                                    </span>
                                </div>

                                <JobStatusPanel
                                    controller=transcription
                                    live_recording_state=live_recording_state
                                    live_recording_label=live_recording_label
                                    live_recording_elapsed_ms=live_recording_elapsed_ms
                                />
                            </div>
                        </div>
                    </section>

                    <TranscriptResultPanel
                        active=active
                        controller=transcription
                        live_recording_state=live_recording_state
                        live_recording_label=live_recording_label
                        live_recording_elapsed_ms=live_recording_elapsed_ms
                    />
                </div>
            </Show>
        </section>
    }
}
