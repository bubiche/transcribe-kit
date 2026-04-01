use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{
    get_settings, list_local_models, pick_audio_file, start_file_transcription,
    write_clipboard_text, AppSettings, InputType, LocalModelDescriptor, ProviderMode,
    TranscriptResult, TranscriptSegment, TranscriptionJobState, TranscriptionJobStatus,
    TranscriptionStreamEvent,
};

#[component]
pub fn TranscribeScreen(active: Signal<bool>) -> impl IntoView {
    let settings = RwSignal::new(AppSettings {
        provider_mode: ProviderMode::Local,
        local_model_id: "whisper-base".to_string(),
        api_model_id: "gpt-4o-mini-transcribe".to_string(),
        api_custom_model_name: String::new(),
        api_base_url: String::new(),
        api_key_present: false,
    });
    let local_models = RwSignal::new(Vec::<LocalModelDescriptor>::new());
    let selected_file = RwSignal::new(None::<String>);
    let transcript = RwSignal::new(None::<TranscriptResult>);
    let streamed_text = RwSignal::new(String::new());
    let streamed_segments = RwSignal::new(Vec::<TranscriptSegment>::new());
    let progress_percent = RwSignal::new(None::<i32>);
    let load_error = RwSignal::new(None::<String>);
    let job_status = RwSignal::new(TranscriptionJobStatus {
        state: TranscriptionJobState::Idle,
        input_type: InputType::File,
        source_name: None,
        message: None,
    });
    let is_loading = RwSignal::new(true);
    let is_transcribing = RwSignal::new(false);

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
        if is_transcribing.get() {
            "Transcribing...".to_string()
        } else {
            "Choose audio file".to_string()
        }
    });

    let on_choose_file = move |_| {
        spawn_local(async move {
            if is_transcribing.get_untracked() {
                return;
            }

            job_status.update(|status| {
                status.message = None;
                if !matches!(status.state, TranscriptionJobState::Succeeded) {
                    status.state = TranscriptionJobState::Idle;
                }
            });

            if settings.get_untracked().provider_mode != ProviderMode::Local {
                transcript.set(None);
                job_status.set(TranscriptionJobStatus {
                    state: TranscriptionJobState::Failed,
                    input_type: InputType::File,
                    source_name: None,
                    message: Some(
                        "Phase 2c is wired to Local Whisper only. Switch the provider in Settings before importing a file."
                            .to_string(),
                    ),
                });
                return;
            }

            if !model_ready.get_untracked() {
                transcript.set(None);
                job_status.set(TranscriptionJobStatus {
                    state: TranscriptionJobState::Failed,
                    input_type: InputType::File,
                    source_name: None,
                    message: Some(
                        "The selected Whisper model is not downloaded yet. Download it from Settings and try again."
                            .to_string(),
                    ),
                });
                return;
            }

            let Some(file_path) = (match pick_audio_file().await {
                Ok(path) => path,
                Err(error) => {
                    job_status.set(TranscriptionJobStatus {
                        state: TranscriptionJobState::Failed,
                        input_type: InputType::File,
                        source_name: None,
                        message: Some(error),
                    });
                    return;
                }
            }) else {
                return;
            };

            let file_name = file_name_from_path(&file_path);
            selected_file.set(Some(file_path.clone()));
            transcript.set(None);
            streamed_text.set(String::new());
            streamed_segments.set(Vec::new());
            progress_percent.set(Some(0));
            is_transcribing.set(true);
            job_status.set(TranscriptionJobStatus {
                state: TranscriptionJobState::Running,
                input_type: InputType::File,
                source_name: Some(file_name.clone()),
                message: Some(format!("Transcribing {file_name}")),
            });

            let progress_job_status = job_status;
            let progress_streamed_text = streamed_text;
            let progress_streamed_segments = streamed_segments;
            let progress_percent_signal = progress_percent;
            let progress_file_name = file_name.clone();

            match start_file_transcription(&file_path, move |event| match event {
                TranscriptionStreamEvent::Progress {
                    progress_percent: next_progress,
                } => {
                    progress_percent_signal.set(Some(next_progress));
                    progress_job_status.update(|status| {
                        status.state = TranscriptionJobState::Running;
                        status.message = Some(format!(
                            "Transcribing {progress_file_name} ({next_progress}%)"
                        ));
                    });
                }
                TranscriptionStreamEvent::Segment {
                    segment_index,
                    segment,
                    accumulated_text,
                    ..
                } => {
                    progress_streamed_text.set(accumulated_text);
                    progress_streamed_segments.update(|segments| {
                        let index = segment_index.max(0) as usize;
                        if index < segments.len() {
                            segments[index] = segment;
                        } else {
                            segments.push(segment);
                        }
                    });
                    progress_job_status.update(|status| {
                        status.state = TranscriptionJobState::Running;
                        status.message =
                            Some("Receiving transcript segments from Whisper...".to_string());
                    });
                }
            })
            .await
            {
                Ok(result) => {
                    transcript.set(Some(result.clone()));
                    streamed_text.set(result.text.clone());
                    streamed_segments.set(result.segments.clone());
                    progress_percent.set(Some(100));
                    job_status.set(TranscriptionJobStatus {
                        state: TranscriptionJobState::Succeeded,
                        input_type: InputType::File,
                        source_name: result.source.source_name.clone(),
                        message: Some("Transcript ready for review.".to_string()),
                    });
                }
                Err(error) => {
                    transcript.set(None);
                    progress_percent.set(None);
                    job_status.set(TranscriptionJobStatus {
                        state: TranscriptionJobState::Failed,
                        input_type: InputType::File,
                        source_name: Some(file_name),
                        message: Some(error),
                    });
                }
            }

            is_transcribing.set(false);
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
                                    disabled=move || is_transcribing.get()
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

                                <JobStatusPanel job_status=job_status />
                            </div>
                        </div>
                    </section>

                    <TranscriptResultPanel
                        transcript=transcript
                        partial_text=streamed_text
                        partial_segments=streamed_segments
                        progress_percent=progress_percent
                        job_status=job_status
                    />
                </div>
            </Show>
        </section>
    }
}

#[component]
fn JobStatusPanel(job_status: RwSignal<TranscriptionJobStatus>) -> impl IntoView {
    let status_class = Signal::derive(move || match job_status.get().state {
        TranscriptionJobState::Idle => "job-status-panel",
        TranscriptionJobState::Running => "job-status-panel running",
        TranscriptionJobState::Succeeded => "job-status-panel success",
        TranscriptionJobState::Failed => "job-status-panel error",
    });

    let status_label = Signal::derive(move || match job_status.get().state {
        TranscriptionJobState::Idle => "Ready",
        TranscriptionJobState::Running => "Transcribing",
        TranscriptionJobState::Succeeded => "Completed",
        TranscriptionJobState::Failed => "Needs attention",
    });

    view! {
        <div class=move || status_class.get()>
            <p class="status-label">{move || status_label.get()}</p>
            <p class="job-status-copy">
                {move || {
                    job_status
                        .get()
                        .message
                        .unwrap_or_else(|| "Pick a file to start a one-shot transcription job.".to_string())
                }}
            </p>
        </div>
    }
}

#[component]
fn TranscriptResultPanel(
    transcript: RwSignal<Option<TranscriptResult>>,
    partial_text: RwSignal<String>,
    partial_segments: RwSignal<Vec<TranscriptSegment>>,
    progress_percent: RwSignal<Option<i32>>,
    job_status: RwSignal<TranscriptionJobStatus>,
) -> impl IntoView {
    let copy_feedback_error = RwSignal::new(false);
    let copy_feedback_target = RwSignal::new(None::<&'static str>);

    let plain_copy_text = Signal::derive(move || {
        transcript
            .get()
            .map(|result| result.text)
            .unwrap_or_else(|| partial_text.get())
    });

    let timestamp_copy_text = Signal::derive(move || {
        if let Some(result) = transcript.get() {
            return format_transcript_with_timestamps(&result.segments, &result.text);
        }

        let current_partial_text = partial_text.get();
        format_transcript_with_timestamps(&partial_segments.get(), &current_partial_text)
    });

    let can_copy = Signal::derive(move || !plain_copy_text.get().trim().is_empty());
    let plain_button_class = Signal::derive(move || {
        if copy_feedback_target.get() == Some("plain") {
            if copy_feedback_error.get() {
                "secondary-button error"
            } else {
                "secondary-button success"
            }
        } else {
            "secondary-button"
        }
    });
    let timestamp_button_class = Signal::derive(move || {
        if copy_feedback_target.get() == Some("timestamps") {
            if copy_feedback_error.get() {
                "secondary-button error"
            } else {
                "secondary-button success"
            }
        } else {
            "secondary-button"
        }
    });
    let plain_button_label = Signal::derive(move || {
        if copy_feedback_target.get() == Some("plain") {
            if copy_feedback_error.get() {
                "Copy failed"
            } else {
                "Copied text"
            }
        } else {
            "Copy text"
        }
    });
    let timestamp_button_label = Signal::derive(move || {
        if copy_feedback_target.get() == Some("timestamps") {
            if copy_feedback_error.get() {
                "Copy failed"
            } else {
                "Copied with timestamps"
            }
        } else {
            "Copy with timestamps"
        }
    });

    Effect::new(move |_| {
        plain_copy_text.get();
        timestamp_copy_text.get();
        copy_feedback_error.set(false);
        copy_feedback_target.set(None);
    });

    let copy_plain = move |_| {
        let text = plain_copy_text.get_untracked();
        if text.trim().is_empty() {
            copy_feedback_error.set(true);
            copy_feedback_target.set(Some("plain"));
            return;
        }

        spawn_local(async move {
            match write_clipboard_text(&text).await {
                Ok(()) => {
                    copy_feedback_error.set(false);
                    copy_feedback_target.set(Some("plain"));
                }
                Err(error) => {
                    copy_feedback_error.set(true);
                    copy_feedback_target.set(Some("plain"));
                    let _ = error;
                }
            }
        });
    };

    let copy_with_timestamps = move |_| {
        let text = timestamp_copy_text.get_untracked();
        if text.trim().is_empty() {
            copy_feedback_error.set(true);
            copy_feedback_target.set(Some("timestamps"));
            return;
        }

        spawn_local(async move {
            match write_clipboard_text(&text).await {
                Ok(()) => {
                    copy_feedback_error.set(false);
                    copy_feedback_target.set(Some("timestamps"));
                }
                Err(error) => {
                    copy_feedback_error.set(true);
                    copy_feedback_target.set(Some("timestamps"));
                    let _ = error;
                }
            }
        });
    };

    view! {
        <section class="section transcript-result-section">
            <p class="tag">"Result"</p>
            <h3>"Transcript review"</h3>
            <div class="result-toolbar">
                <div class="copy-actions">
                    <button
                        class=move || plain_button_class.get()
                        on:click=copy_plain
                        disabled=move || !can_copy.get()
                    >
                        {move || plain_button_label.get()}
                    </button>
                    <button
                        class=move || timestamp_button_class.get()
                        on:click=copy_with_timestamps
                        disabled=move || !can_copy.get()
                    >
                        {move || timestamp_button_label.get()}
                    </button>
                </div>
            </div>

            <Show
                when=move || transcript.get().is_some()
                fallback=move || {
                    view! {
                        <Show
                            when=move || !partial_text.get().is_empty()
                            fallback=move || {
                                view! {
                                    <p class="body-copy">
                                        {move || {
                                            match job_status.get().state {
                                                TranscriptionJobState::Failed => {
                                                    job_status
                                                        .get()
                                                        .message
                                                        .unwrap_or_else(|| "Transcription failed.".to_string())
                                                }
                                                _ => "Your transcript will appear here once a file finishes processing.".to_string(),
                                            }
                                        }}
                                    </p>
                                }
                            }
                        >
                            <div class="stack">
                                <div class="mini-status">
                                    <span class="mini-chip">
                                        {move || {
                                            if matches!(job_status.get().state, TranscriptionJobState::Failed) {
                                                "Partial draft"
                                            } else {
                                                "Live draft"
                                            }
                                        }}
                                    </span>
                                    <Show when=move || progress_percent.get().is_some()>
                                        <span class="mini-chip">
                                            {move || format!("Progress: {}%", progress_percent.get().unwrap_or(0))}
                                        </span>
                                    </Show>
                                </div>

                                <Show when=move || matches!(job_status.get().state, TranscriptionJobState::Failed)>
                                    <p class="body-copy">
                                        {move || {
                                            job_status
                                                .get()
                                                .message
                                                .unwrap_or_else(|| "Transcription failed after producing a partial draft.".to_string())
                                        }}
                                    </p>
                                </Show>

                                <div class="transcript-output">{move || partial_text.get()}</div>

                                <Show when=move || !partial_segments.get().is_empty()>
                                    <div class="segments-list">
                                        <For
                                            each=move || partial_segments.get()
                                            key=|segment| format!("{}-{}", segment.start_ms, segment.end_ms)
                                            children=move |segment| {
                                                view! {
                                                    <div class="segment-row">
                                                        <span class="segment-time">
                                                            {format_timestamp(segment.start_ms)}
                                                            " - "
                                                            {format_timestamp(segment.end_ms)}
                                                        </span>
                                                        <span class="segment-text">{segment.text}</span>
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                </Show>
                            </div>
                        </Show>
                    }
                }
            >
                {move || {
                    transcript.get().map(|result| {
                        let duration = result
                            .source
                            .duration_ms
                            .map(format_duration_label)
                            .unwrap_or_else(|| "Unknown duration".to_string());
                        let source_name = result
                            .source
                            .source_name
                            .clone()
                            .unwrap_or_else(|| "Imported file".to_string());
                        let provider = result.source.provider.clone();
                        let model_id = result.source.model_id.clone();
                        let text = result.text.clone();
                        let segments = result.segments.clone();
                        let segments_view = if segments.is_empty() {
                            ().into_any()
                        } else {
                            view! {
                                <div class="segments-list">
                                    <For
                                        each=move || segments.clone()
                                        key=|segment| format!("{}-{}", segment.start_ms, segment.end_ms)
                                        children=move |segment| {
                                            view! {
                                                <div class="segment-row">
                                                    <span class="segment-time">
                                                        {format_timestamp(segment.start_ms)}
                                                        " - "
                                                        {format_timestamp(segment.end_ms)}
                                                    </span>
                                                    <span class="segment-text">{segment.text}</span>
                                                </div>
                                            }
                                        }
                                    />
                                </div>
                            }
                            .into_any()
                        };

                        view! {
                            <div class="stack">
                                <div class="mini-status">
                                    <span class="mini-chip">{source_name}</span>
                                    <span class="mini-chip">{format!("Engine: {provider}")}</span>
                                    <span class="mini-chip">{format!("Model: {model_id}")}</span>
                                    <span class="mini-chip">{duration}</span>
                                </div>

                                <div class="transcript-output">{text}</div>
                                {segments_view}
                            </div>
                        }
                            .into_any()
                    })
                }}
            </Show>
        </section>
    }
}

fn file_name_from_path(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn format_duration_label(duration_ms: u64) -> String {
    format!("Duration: {}", format_timestamp(duration_ms as i64))
}

fn format_transcript_with_timestamps(
    segments: &[TranscriptSegment],
    fallback_text: &str,
) -> String {
    if segments.is_empty() {
        return fallback_text.trim().to_string();
    }

    segments
        .iter()
        .map(|segment| {
            format!(
                "[{} - {}] {}",
                format_timestamp(segment.start_ms),
                format_timestamp(segment.end_ms),
                segment.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_timestamp(milliseconds: i64) -> String {
    let total_seconds = (milliseconds.max(0) / 1000) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_transcript_with_timestamps_falls_back_to_plain_text_without_segments() {
        let formatted = format_transcript_with_timestamps(&[], "  Hello world  ");

        assert_eq!(formatted, "Hello world");
    }

    #[test]
    fn format_transcript_with_timestamps_renders_timestamped_lines() {
        let segments = vec![
            TranscriptSegment {
                start_ms: 0,
                end_ms: 3_000,
                text: " Hello ".to_string(),
            },
            TranscriptSegment {
                start_ms: 3_000,
                end_ms: 7_000,
                text: "world".to_string(),
            },
        ];

        let formatted = format_transcript_with_timestamps(&segments, "ignored");

        assert_eq!(formatted, "[00:00 - 00:03] Hello\n[00:03 - 00:07] world");
    }
}
