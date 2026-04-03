mod controller;
mod panels;
mod utils;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{
    get_settings, list_local_models, pick_audio_file, start_file_transcription, AppSettings,
    InputType, LocalModelDescriptor, ProviderMode,
};

pub use self::controller::TranscriptionController;
pub use self::panels::{JobStatusPanel, TranscriptResultPanel};
use self::utils::file_name_from_path;

#[component]
pub fn TranscribeScreen(active: Signal<bool>) -> impl IntoView {
    let settings = RwSignal::new(AppSettings::default());
    let local_models = RwSignal::new(Vec::<LocalModelDescriptor>::new());
    let selected_file = RwSignal::new(None::<String>);
    let load_error = RwSignal::new(None::<String>);
    let is_loading = RwSignal::new(true);
    let transcription = TranscriptionController::new();

    Effect::new(move |_| {
        if !active.get() {
            return;
        }

        spawn_local(async move {
            is_loading.set(true);
            load_error.set(None);

            let settings_result = get_settings().await;
            let models_result = list_local_models().await;

            match (settings_result, models_result) {
                (Ok(loaded_settings), Ok(models)) => {
                    settings.set(loaded_settings);
                    local_models.set(models);
                }
                (settings_result, models_result) => {
                    let mut problems = Vec::new();

                    if let Err(error) = settings_result {
                        problems.push(format!("settings: {error}"));
                    }
                    if let Err(error) = models_result {
                        problems.push(format!("local models: {error}"));
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

    let action_button_label = Signal::derive(move || {
        if transcription.is_transcribing.get() {
            "Transcribing...".to_string()
        } else {
            "Choose audio file".to_string()
        }
    });

    let on_choose_file = move |_| {
        spawn_local(async move {
            if transcription.is_transcribing.get_untracked() {
                return;
            }

            transcription.reset_job_feedback();

            if settings.get_untracked().provider_mode != ProviderMode::Local {
                transcription.set_preflight_failure(
                    InputType::File,
                    "Phase 2c is wired to Local Whisper only. Switch the provider in Settings before importing a file.",
                );
                return;
            }

            if !model_ready.get_untracked() {
                transcription.set_preflight_failure(
                    InputType::File,
                    "The selected Whisper model is not downloaded yet. Download it from Settings and try again.",
                );
                return;
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
                <h2>"File transcription"</h2>
                <p>
                    "Import an audio file, run it through the selected local Whisper model, and review the transcript in-app."
                </p>
            </div>

            <div class="status">
                <div class="status-card">
                    <p class="status-label">"Provider"</p>
                    <p class="status-value">{move || provider_label.get()}</p>
                </div>
                <div class="status-card">
                    <p class="status-label">"Whisper model"</p>
                    <p class="status-value">
                        {move || {
                            selected_model
                                .get()
                                .map(|model| model.label)
                                .unwrap_or_else(|| settings.get().local_model_id)
                        }}
                    </p>
                </div>
                <div class="status-card">
                    <p class="status-label">"Model status"</p>
                    <p class="status-value">
                        {move || if model_ready.get() { "Ready" } else { "Download required" }}
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
                    <p class="body-copy">"Fetching saved settings and local model metadata."</p>
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
                                <p class="tag">"Import"</p>
                                <h3>"Choose a local audio file"</h3>
                                <p class="body-copy">
                                    "Supported import formats: WAV, MP3, FLAC, OGG, and M4A. Files are decoded locally and sent straight to Whisper."
                                </p>
                            </div>

                            <div class="import-actions">
                                <button
                                    class="primary-button"
                                    on:click=on_choose_file
                                    disabled=move || transcription.is_transcribing.get()
                                >
                                    {move || action_button_label.get()}
                                </button>

                                <div class="mini-status">
                                    <span class="mini-chip">
                                        {move || format!("Provider: {}", provider_label.get())}
                                    </span>
                                    <span class="mini-chip">
                                        {move || {
                                            let label = selected_model
                                                .get()
                                                .map(|model| model.label)
                                                .unwrap_or_else(|| settings.get().local_model_id);
                                            format!("Model: {label}")
                                        }}
                                    </span>
                                </div>

                                <JobStatusPanel controller=transcription />
                            </div>
                        </div>
                    </section>

                    <TranscriptResultPanel controller=transcription />
                </div>
            </Show>
        </section>
    }
}
