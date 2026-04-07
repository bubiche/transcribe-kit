use leptos::html::Section;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::features::navigation::Screen;
use crate::live_recording::format_duration;
use crate::tauri_api::LiveRecordingState;
use crate::tauri_api::{
    InputType, LiveCaptureProfile, TranscriptResult, TranscriptionJobState, TranscriptionJobStatus,
    TranscriptionSource,
};

use super::controller::TranscriptionController;
use super::utils::{format_duration_label, format_timestamp, format_transcript_with_timestamps};

#[component]
pub fn JobStatusPanel(
    controller: TranscriptionController,
    live_recording_state: Signal<LiveRecordingState>,
    live_recording_label: Signal<String>,
    live_recording_elapsed_ms: Signal<u64>,
) -> impl IntoView {
    let is_listening =
        Signal::derive(move || matches!(live_recording_state.get(), LiveRecordingState::Recording));

    let status_class = Signal::derive(move || {
        if is_listening.get() {
            "job-status-panel running"
        } else {
            match controller.job_status.get().state {
                TranscriptionJobState::Idle => "job-status-panel",
                TranscriptionJobState::Running => "job-status-panel running",
                TranscriptionJobState::Succeeded => "job-status-panel success",
                TranscriptionJobState::Failed => "job-status-panel error",
            }
        }
    });

    let status_label = Signal::derive(move || {
        if is_listening.get() {
            "Listening"
        } else {
            match controller.job_status.get().state {
                TranscriptionJobState::Idle => "Ready",
                TranscriptionJobState::Running => "Transcribing",
                TranscriptionJobState::Succeeded => "Completed",
                TranscriptionJobState::Failed => "Needs attention",
            }
        }
    });

    view! {
        <div class=move || status_class.get()>
            <p class="status-label">{move || status_label.get()}</p>
            <p class="job-status-copy">
                {move || {
                    if is_listening.get() {
                        listening_status_message(
                            &live_recording_label.get(),
                            live_recording_elapsed_ms.get(),
                        )
                    } else {
                        controller
                            .job_status
                            .get()
                            .message
                            .unwrap_or_else(|| {
                                "Choose a file or start live capture to begin a transcription job."
                                    .to_string()
                            })
                    }
                }}
            </p>
        </div>
    }
}

#[component]
pub fn TranscriptResultPanel(
    active: Signal<bool>,
    controller: TranscriptionController,
    active_screen: RwSignal<Screen>,
    live_recording_state: Signal<LiveRecordingState>,
    live_recording_label: Signal<String>,
    live_recording_elapsed_ms: Signal<u64>,
) -> impl IntoView {
    let panel_ref = NodeRef::<Section>::new();
    let copy_feedback_error = RwSignal::new(false);
    let copy_feedback_target = RwSignal::new(None::<&'static str>);
    let last_focused_completion_nonce = RwSignal::new(None::<u64>);
    let highlighted_completion_nonce = RwSignal::new(None::<u64>);
    let is_listening =
        Signal::derive(move || matches!(live_recording_state.get(), LiveRecordingState::Recording));

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

    let has_segments = Signal::derive(move || {
        controller
            .transcript
            .get()
            .map(|r| !r.segments.is_empty())
            .unwrap_or_else(|| !controller.partial_segments.get().is_empty())
    });

    let can_copy =
        Signal::derive(move || can_copy_transcript(is_listening.get(), &plain_copy_text.get()));
    let show_postprocess_button = Signal::derive(move || {
        !is_listening.get()
            && !controller.is_transcribing.get()
            && controller.transcript.with(|opt| opt.is_some())
    });
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

    Effect::new(move |_| {
        let job_status = controller.job_status.get();
        let transcript = controller.transcript.get();
        let completion_nonce = controller.completion_nonce.get();
        let last_focused_nonce = last_focused_completion_nonce.get();
        if should_focus_live_transcript(
            active.get(),
            is_listening.get(),
            &job_status,
            transcript.as_ref(),
            completion_nonce,
            last_focused_nonce,
        ) {
            if let Some(panel) = panel_ref.get() {
                panel.scroll_into_view();
                let _ = panel.focus();
            }
            last_focused_completion_nonce.set(Some(completion_nonce));
            trigger_live_completion_highlight(highlighted_completion_nonce, completion_nonce);
        }
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
        <section
            node_ref=panel_ref
            tabindex="-1"
            class="section transcript-result-section"
            class:transcript-result-section-live-complete=move || {
                highlighted_completion_nonce.get() == Some(controller.completion_nonce.get())
            }
        >
            <p class="tag">"Result"</p>
            <h3>"Transcript review"</h3>
            <Show when=move || {
                highlighted_completion_nonce.get() == Some(controller.completion_nonce.get())
                    && !is_listening.get()
            }>
                <div class="transcript-success-banner" role="status" aria-live="polite">
                    "Live transcript ready"
                </div>
            </Show>
            <Show
                when=move || is_listening.get()
                fallback=move || {
                    view! { <TranscriptPanelBody controller=controller /> }
                }
            >
                <ListeningIndicator
                    live_recording_label=live_recording_label
                    live_recording_elapsed_ms=live_recording_elapsed_ms
                />
            </Show>

            <div class="result-toolbar">
                <div class="copy-actions">
                    <button
                        class=move || plain_button_class.get()
                        on:click=copy_plain
                        disabled=move || !can_copy.get()
                    >
                        {move || plain_button_label.get()}
                    </button>
                    <Show when=move || has_segments.get()>
                        <button
                            class=move || timestamp_button_class.get()
                            on:click=copy_with_timestamps
                            disabled=move || !can_copy.get()
                        >
                            {move || timestamp_button_label.get()}
                        </button>
                    </Show>
                    <Show when=move || show_postprocess_button.get()>
                        <button
                            class="secondary-button postprocess-nav-button"
                            on:click=move |_| active_screen.set(Screen::PostProcess)
                        >
                            "Post-process"
                        </button>
                    </Show>
                </div>
            </div>
        </section>
    }
}

#[component]
fn TranscriptPanelBody(controller: TranscriptionController) -> impl IntoView {
    view! {
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
    }
}

#[component]
fn ListeningIndicator(
    live_recording_label: Signal<String>,
    live_recording_elapsed_ms: Signal<u64>,
) -> impl IntoView {
    view! {
        <div class="listening-indicator" role="status" aria-live="polite">
            <div class="listening-indicator-row">
                <span class="listening-indicator-badge">
                    <span class="listening-indicator-dot"></span>
                    "Listening"
                </span>
                <span class="listening-indicator-wave" aria-hidden="true">
                    <span></span>
                    <span></span>
                    <span></span>
                </span>
            </div>
            <div class="mini-status">
                <span class="mini-chip">{move || format!("Input: {}", live_recording_label.get())}</span>
                <span class="mini-chip">
                    {move || format!("Elapsed: {}", format_duration(live_recording_elapsed_ms.get()))}
                </span>
            </div>
            <p class="body-copy">
                "Your audio input is active. Release the hotkey or press it again to stop and generate the transcript."
            </p>
        </div>
    }
}

fn listening_status_message(input_label: &str, elapsed_ms: u64) -> String {
    format!(
        "Recording from {input_label}. Stop recording to generate the transcript. Elapsed: {}.",
        format_duration(elapsed_ms)
    )
}

fn can_copy_transcript(is_listening: bool, text: &str) -> bool {
    !is_listening && !text.trim().is_empty()
}

fn trigger_live_completion_highlight(
    highlighted_completion_nonce: RwSignal<Option<u64>>,
    completion_nonce: u64,
) {
    highlighted_completion_nonce.set(Some(completion_nonce));

    let clear_signal = highlighted_completion_nonce;
    let timeout_closure = wasm_bindgen::closure::Closure::once_into_js(move || {
        if clear_signal.get_untracked() == Some(completion_nonce) {
            clear_signal.set(None);
        }
    });

    if let Some(window) = web_sys::window() {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timeout_closure.as_ref().unchecked_ref(),
            2400,
        );
    }
}

fn should_focus_live_transcript(
    active: bool,
    is_listening: bool,
    job_status: &TranscriptionJobStatus,
    transcript: Option<&TranscriptResult>,
    completion_nonce: u64,
    last_focused_completion_nonce: Option<u64>,
) -> bool {
    active
        && !is_listening
        && matches!(job_status.state, TranscriptionJobState::Succeeded)
        && matches!(job_status.input_type, InputType::Live)
        && last_focused_completion_nonce != Some(completion_nonce)
        && transcript
            .map(|result| matches!(result.source.input_type, InputType::Live))
            .unwrap_or(false)
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
    let source_name = primary_source_label(&result.source);
    let input_label = supporting_input_label(&result.source);
    let input_chip = if let Some(label) = input_label {
        view! { <span class="mini-chip">{label}</span> }.into_any()
    } else {
        ().into_any()
    };
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
                {input_chip}
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

fn primary_source_label(source: &TranscriptionSource) -> String {
    match source.input_type {
        InputType::File => source
            .source_name
            .clone()
            .unwrap_or_else(|| source_name_fallback(InputType::File, None)),
        InputType::Live => source_name_fallback(InputType::Live, source.live_capture_profile),
    }
}

fn supporting_input_label(source: &TranscriptionSource) -> Option<String> {
    if !matches!(source.input_type, InputType::Live) {
        return None;
    }

    normalized_source_name(source.source_name.as_deref()).map(|label| format!("Input: {label}"))
}

fn normalized_source_name(source_name: Option<&str>) -> Option<String> {
    source_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn source_name_fallback(
    input_type: InputType,
    live_capture_profile: Option<LiveCaptureProfile>,
) -> String {
    match (input_type, live_capture_profile) {
        (InputType::File, _) => "Imported file".to_string(),
        (InputType::Live, Some(LiveCaptureProfile::MicrophoneOnly)) => {
            "Live microphone note".to_string()
        }
        (InputType::Live, Some(LiveCaptureProfile::MeetingMix)) => {
            "Live meeting capture".to_string()
        }
        (InputType::Live, None) => "Live recording".to_string(),
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
        assert_eq!(source_name_fallback(InputType::File, None), "Imported file");
        assert_eq!(
            source_name_fallback(InputType::Live, None),
            "Live recording"
        );
        assert_eq!(
            source_name_fallback(InputType::Live, Some(LiveCaptureProfile::MicrophoneOnly),),
            "Live microphone note"
        );
        assert_eq!(
            source_name_fallback(InputType::Live, Some(LiveCaptureProfile::MeetingMix)),
            "Live meeting capture"
        );
    }

    #[test]
    fn primary_source_label_prefers_capture_label_for_live_results() {
        assert_eq!(
            primary_source_label(&crate::tauri_api::TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::Live,
                live_capture_profile: Some(LiveCaptureProfile::MeetingMix),
                source_name: Some("Desk Mic".to_string()),
                duration_ms: Some(1_000),
            }),
            "Live meeting capture"
        );
        assert_eq!(
            primary_source_label(&crate::tauri_api::TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: Some("note.wav".to_string()),
                duration_ms: Some(1_000),
            }),
            "note.wav"
        );
    }

    #[test]
    fn supporting_input_label_is_only_shown_for_live_results_with_device_names() {
        assert_eq!(
            supporting_input_label(&crate::tauri_api::TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::Live,
                live_capture_profile: Some(LiveCaptureProfile::MeetingMix),
                source_name: Some(" Desk Mic ".to_string()),
                duration_ms: Some(1_000),
            }),
            Some("Input: Desk Mic".to_string())
        );
        assert_eq!(
            supporting_input_label(&crate::tauri_api::TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: Some("note.wav".to_string()),
                duration_ms: Some(1_000),
            }),
            None
        );
    }

    #[test]
    fn can_copy_transcript_is_disabled_while_listening() {
        assert!(can_copy_transcript(false, "hello world"));
        assert!(!can_copy_transcript(true, "hello world"));
        assert!(!can_copy_transcript(false, "   "));
    }

    #[test]
    fn listening_status_message_includes_source_and_elapsed_time() {
        assert_eq!(
            listening_status_message("Desk Mic", 5_200),
            "Recording from Desk Mic. Stop recording to generate the transcript. Elapsed: 00:05."
        );
    }

    #[test]
    fn should_focus_live_transcript_only_for_active_live_success() {
        let live_result = TranscriptResult {
            text: "hello".to_string(),
            segments: Vec::new(),
            source: crate::tauri_api::TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::Live,
                live_capture_profile: Some(LiveCaptureProfile::MicrophoneOnly),
                source_name: Some("Desk Mic".to_string()),
                duration_ms: Some(1_000),
            },
            post_processed_text: None,
        };

        let live_status = TranscriptionJobStatus {
            state: TranscriptionJobState::Succeeded,
            input_type: InputType::Live,
            source_name: Some("Desk Mic".to_string()),
            message: Some("Transcript ready for review.".to_string()),
        };

        assert!(should_focus_live_transcript(
            true,
            false,
            &live_status,
            Some(&live_result),
            3,
            Some(2),
        ));
        assert!(!should_focus_live_transcript(
            false,
            false,
            &live_status,
            Some(&live_result),
            3,
            Some(2),
        ));
        assert!(!should_focus_live_transcript(
            true,
            true,
            &live_status,
            Some(&live_result),
            3,
            Some(2),
        ));
        assert!(!should_focus_live_transcript(
            true,
            false,
            &live_status,
            Some(&live_result),
            3,
            Some(3),
        ));
    }

    #[test]
    fn should_focus_live_transcript_rejects_file_input_type_in_result() {
        let file_result = TranscriptResult {
            text: "file transcript".to_string(),
            segments: Vec::new(),
            source: crate::tauri_api::TranscriptionSource {
                provider: "openai-compatible".to_string(),
                model_id: "gpt-4o-mini-transcribe".to_string(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: Some("note.wav".to_string()),
                duration_ms: Some(2_000),
            },
            post_processed_text: None,
        };

        let live_status = TranscriptionJobStatus {
            state: TranscriptionJobState::Succeeded,
            input_type: InputType::Live,
            source_name: Some("Desk Mic".to_string()),
            message: Some("Transcript ready for review.".to_string()),
        };

        assert!(
            !should_focus_live_transcript(
                true,
                false,
                &live_status,
                Some(&file_result),
                3,
                Some(2),
            ),
            "should not focus when result has File input type"
        );
    }

    #[test]
    fn source_name_fallback_api_provider_uses_same_labels() {
        // API transcripts should use the same source labels as local.
        // This verifies that the label logic is provider-neutral.
        assert_eq!(source_name_fallback(InputType::File, None), "Imported file");
        assert_eq!(
            source_name_fallback(InputType::Live, Some(LiveCaptureProfile::MicrophoneOnly)),
            "Live microphone note"
        );
    }
}
