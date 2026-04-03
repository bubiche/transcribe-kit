use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{InputType, TranscriptResult, TranscriptionJobState};

use super::controller::TranscriptionController;
use super::utils::{format_duration_label, format_timestamp, format_transcript_with_timestamps};

#[component]
pub fn JobStatusPanel(controller: TranscriptionController) -> impl IntoView {
    let status_class = Signal::derive(move || match controller.job_status.get().state {
        TranscriptionJobState::Idle => "job-status-panel",
        TranscriptionJobState::Running => "job-status-panel running",
        TranscriptionJobState::Succeeded => "job-status-panel success",
        TranscriptionJobState::Failed => "job-status-panel error",
    });

    let status_label = Signal::derive(move || match controller.job_status.get().state {
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
                    controller
                        .job_status
                        .get()
                        .message
                        .unwrap_or_else(|| {
                            "Choose a file or start live capture to begin a transcription job."
                                .to_string()
                        })
                }}
            </p>
        </div>
    }
}

#[component]
pub fn TranscriptResultPanel(controller: TranscriptionController) -> impl IntoView {
    let copy_feedback_error = RwSignal::new(false);
    let copy_feedback_target = RwSignal::new(None::<&'static str>);

    let plain_copy_text = Signal::derive(move || {
        controller
            .transcript
            .get()
            .map(|result| result.text)
            .unwrap_or_else(|| controller.partial_text.get())
    });

    let timestamp_copy_text = Signal::derive(move || {
        if let Some(result) = controller.transcript.get() {
            return format_transcript_with_timestamps(&result.segments, &result.text);
        }

        let current_partial_text = controller.partial_text.get();
        format_transcript_with_timestamps(&controller.partial_segments.get(), &current_partial_text)
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
            match crate::tauri_api::write_clipboard_text(&text).await {
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
            match crate::tauri_api::write_clipboard_text(&text).await {
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
                when=move || controller.transcript.get().is_some()
                fallback=move || {
                    view! {
                        <Show
                            when=move || !controller.partial_text.get().is_empty()
                            fallback=move || {
                                view! {
                                    <p class="body-copy">
                                        {move || {
                                            match controller.job_status.get().state {
                                                TranscriptionJobState::Failed => {
                                                    controller
                                                        .job_status
                                                        .get()
                                                        .message
                                                        .unwrap_or_else(|| "Transcription failed.".to_string())
                                                }
                                                _ => "Your transcript will appear here once audio finishes processing.".to_string(),
                                            }
                                        }}
                                    </p>
                                }
                            }
                        >
                            <PartialTranscriptPanel controller=controller />
                        </Show>
                    }
                }
            >
                {move || controller.transcript.get().map(render_transcript_result)}
            </Show>
        </section>
    }
}

#[component]
fn PartialTranscriptPanel(controller: TranscriptionController) -> impl IntoView {
    view! {
        <div class="stack">
            <div class="mini-status">
                <span class="mini-chip">
                    {move || draft_label(controller.job_status.get().state, controller.job_status.get().input_type)}
                </span>
                <Show when=move || controller.progress_percent.get().is_some()>
                    <span class="mini-chip">
                        {move || {
                            format!(
                                "Progress: {}%",
                                controller.progress_percent.get().unwrap_or(0)
                            )
                        }}
                    </span>
                </Show>
            </div>

            <Show when=move || matches!(controller.job_status.get().state, TranscriptionJobState::Failed)>
                <p class="body-copy">
                    {move || {
                        controller
                            .job_status
                            .get()
                            .message
                            .unwrap_or_else(|| "Transcription failed after producing a partial draft.".to_string())
                    }}
                </p>
            </Show>

            <div class="transcript-output">{move || controller.partial_text.get()}</div>

            <Show when=move || !controller.partial_segments.get().is_empty()>
                <div class="segments-list">
                    <For
                        each=move || controller.partial_segments.get()
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
    }
}

fn render_transcript_result(result: TranscriptResult) -> impl IntoView {
    let duration = result
        .source
        .duration_ms
        .map(format_duration_label)
        .unwrap_or_else(|| "Unknown duration".to_string());
    let source_name = result
        .source
        .source_name
        .clone()
        .unwrap_or_else(|| source_name_fallback(result.source.input_type));
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
}

fn draft_label(state: TranscriptionJobState, input_type: InputType) -> &'static str {
    if matches!(state, TranscriptionJobState::Failed) {
        "Partial draft"
    } else {
        match input_type {
            InputType::File => "File draft",
            InputType::Live => "Live draft",
        }
    }
}

fn source_name_fallback(input_type: InputType) -> String {
    match input_type {
        InputType::File => "Imported file".to_string(),
        InputType::Live => "Live recording".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_label_matches_active_input_type() {
        assert_eq!(
            draft_label(TranscriptionJobState::Running, InputType::File),
            "File draft"
        );
        assert_eq!(
            draft_label(TranscriptionJobState::Running, InputType::Live),
            "Live draft"
        );
    }

    #[test]
    fn draft_label_prefers_partial_for_failures() {
        assert_eq!(
            draft_label(TranscriptionJobState::Failed, InputType::File),
            "Partial draft"
        );
        assert_eq!(
            draft_label(TranscriptionJobState::Failed, InputType::Live),
            "Partial draft"
        );
    }

    #[test]
    fn source_name_fallback_matches_input_type() {
        assert_eq!(source_name_fallback(InputType::File), "Imported file");
        assert_eq!(source_name_fallback(InputType::Live), "Live recording");
    }
}
