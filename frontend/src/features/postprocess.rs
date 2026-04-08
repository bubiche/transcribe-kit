use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::features::transcription::format_timestamp;
use crate::features::transcription::TranscriptionController;
use crate::tauri_api::{
    list_templates, run_postprocess, save_templates, write_clipboard_text, PostProcessTemplate,
    TranscriptResult,
};

#[component]
pub fn PostProcessScreen(
    active: Signal<bool>,
    transcription: TranscriptionController,
) -> impl IntoView {
    let templates = RwSignal::new(Vec::<PostProcessTemplate>::new());
    let selected_template_id = RwSignal::new(None::<String>);
    let editing_name = RwSignal::new(String::new());
    let editing_prompt = RwSignal::new(String::new());
    let is_new_template = RwSignal::new(false);
    let save_feedback = RwSignal::new(None::<String>);

    let is_running = RwSignal::new(false);
    let result_text = RwSignal::new(None::<String>);
    let error_message = RwSignal::new(None::<String>);
    let copy_feedback = RwSignal::new(None::<&'static str>);

    let apply_selection = move |id: Option<String>, source: &[PostProcessTemplate]| {
        selected_template_id.set(id.clone());
        is_new_template.set(false);
        match id.and_then(|id| source.iter().find(|t| t.id == id)) {
            Some(tpl) => {
                editing_name.set(tpl.name.clone());
                editing_prompt.set(tpl.prompt.clone());
            }
            None => {
                editing_name.set(String::new());
                editing_prompt.set(String::new());
            }
        }
    };

    Effect::new(move |_| {
        if !active.get() {
            return;
        }

        save_feedback.set(None);

        spawn_local(async move {
            match list_templates().await {
                Ok(loaded) => {
                    let previous_id = selected_template_id.get_untracked();
                    templates.set(loaded.clone());

                    let still_exists = previous_id
                        .as_ref()
                        .map(|id| loaded.iter().any(|t| &t.id == id))
                        .unwrap_or(false);

                    let effective_id = if still_exists {
                        previous_id
                    } else {
                        loaded.first().map(|t| t.id.clone())
                    };
                    apply_selection(effective_id, &loaded);
                }
                Err(_) => {
                    templates.set(Vec::new());
                    apply_selection(None, &[]);
                }
            }
        });
    });

    let on_select_template = move |event: leptos::ev::Event| {
        let value = event_target_value(&event);
        save_feedback.set(None);
        let id = if value.is_empty() { None } else { Some(value) };
        apply_selection(id, &templates.get_untracked());
    };

    let on_new_template = move |_| {
        let new_id = generate_template_id();
        selected_template_id.set(Some(new_id));
        editing_name.set(String::new());
        editing_prompt.set(String::new());
        is_new_template.set(true);
        save_feedback.set(None);
    };

    let on_save_template = move |_| {
        let Some(id) = selected_template_id.get_untracked() else {
            return;
        };
        let name = editing_name.get_untracked();
        let prompt = editing_prompt.get_untracked();

        if name.trim().is_empty() {
            save_feedback.set(Some("Template name cannot be empty.".to_string()));
            return;
        }

        let updated_template = PostProcessTemplate {
            id: id.clone(),
            name,
            prompt,
        };

        let mut current = templates.get_untracked();
        if let Some(existing) = current.iter_mut().find(|t| t.id == id) {
            *existing = updated_template;
        } else {
            current.push(updated_template);
        }

        let to_save = current.clone();
        templates.set(current);
        is_new_template.set(false);
        save_feedback.set(None);

        spawn_local(async move {
            match save_templates(to_save).await {
                Ok(()) => {
                    save_feedback.set(Some("Template saved.".to_string()));
                }
                Err(error) => {
                    save_feedback.set(Some(format!("Save failed: {error}")));
                }
            }
        });
    };

    let on_delete_template = move |_| {
        let Some(id) = selected_template_id.get_untracked() else {
            return;
        };
        let mut current = templates.get_untracked();
        current.retain(|t| t.id != id);
        let to_save = current.clone();
        templates.set(current.clone());
        save_feedback.set(None);

        let next_id = current.first().map(|t| t.id.clone());
        apply_selection(next_id, &current);

        spawn_local(async move {
            if let Err(error) = save_templates(to_save).await {
                save_feedback.set(Some(format!("Delete failed to persist: {error}")));
            }
        });
    };

    let prompt_missing_placeholder = Signal::derive(move || {
        let prompt = editing_prompt.get();
        !prompt.is_empty() && !prompt.contains("{{transcript}}")
    });

    let has_templates = Signal::derive(move || !templates.get().is_empty());
    let has_selection = Signal::derive(move || selected_template_id.get().is_some());

    let transcript_signal = Signal::derive(move || transcription.transcript.get());
    let transcript_text = Signal::derive(move || {
        transcript_signal
            .get()
            .map(|r| r.text.clone())
            .unwrap_or_default()
    });
    let has_transcript = Signal::derive(move || {
        transcript_signal
            .get()
            .map(|r| !r.text.trim().is_empty())
            .unwrap_or(false)
    });

    let had_result_on_load = RwSignal::new(false);

    // Track whether the transcript already had a post-processed result when
    // the screen was activated (i.e., from a previous run, not the current one).
    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        let has = transcription.transcript.with(|opt| {
            opt.as_ref()
                .and_then(|r| r.post_processed_text.as_ref())
                .is_some()
        });
        had_result_on_load.set(has);
    });

    let can_run =
        Signal::derive(move || has_transcript.get() && has_selection.get() && !is_running.get());

    let run_button_label = Signal::derive(move || {
        if is_running.get() {
            "Processing..."
        } else {
            "Run post-processing"
        }
    });

    let on_run_postprocess = move |_| {
        let Some(template_id) = selected_template_id.get_untracked() else {
            return;
        };
        let text = transcript_text.get_untracked();
        if text.trim().is_empty() {
            return;
        }

        is_running.set(true);
        error_message.set(None);
        result_text.set(None);
        copy_feedback.set(None);

        spawn_local(async move {
            match run_postprocess(text, template_id).await {
                Ok(processed) => {
                    result_text.set(Some(processed.clone()));
                    // Write back to TranscriptionController
                    transcription.transcript.update(|opt| {
                        if let Some(result) = opt.as_mut() {
                            result.post_processed_text = Some(processed);
                        }
                    });
                }
                Err(err) => {
                    error_message.set(Some(err));
                }
            }
            is_running.set(false);
        });
    };

    // Track which transcription the current postprocess result belongs to.
    // When a new transcription completes (nonce changes), clear stale state.
    let last_seen_nonce = RwSignal::new(transcription.completion_nonce.get_untracked());

    Effect::new(move |_| {
        let current_nonce = transcription.completion_nonce.get();
        let previous_nonce = last_seen_nonce.get_untracked();

        if current_nonce != previous_nonce {
            last_seen_nonce.set(current_nonce);
            result_text.set(None);
            error_message.set(None);
            copy_feedback.set(None);
        }
    });

    // Restore previously processed text when navigating back
    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        if result_text.get_untracked().is_some() {
            return;
        }
        if let Some(existing) = transcription
            .transcript
            .get_untracked()
            .and_then(|r| r.post_processed_text)
        {
            result_text.set(Some(existing));
        }
    });

    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Post-processing"</h2>
                <p>"Apply prompt-driven cleanup, formatting, and rewriting to transcripts."</p>
            </div>

            <div class="postprocess-grid">
                <div class="postprocess-editor-column">
                    <section class="section">
                        <p class="tag">"Templates"</p>
                        <h3>"Prompt templates"</h3>

                        <div class="stack">
                            <label class="field">
                                <span class="field-label">"Template"</span>
                                <select
                                    prop:value=move || selected_template_id.get().unwrap_or_default()
                                    on:change=on_select_template
                                >
                                    <Show when=move || !has_templates.get() && !is_new_template.get()>
                                        <option value="">"No templates available"</option>
                                    </Show>
                                    <For
                                        each=move || templates.get()
                                        key=|tpl| tpl.id.clone()
                                        children=move |tpl| {
                                            view! {
                                                <option value=tpl.id.clone()>{tpl.name.clone()}</option>
                                            }
                                        }
                                    />
                                    <Show when=move || is_new_template.get()>
                                        <option value=move || selected_template_id.get().unwrap_or_default()>
                                            "New template (unsaved)"
                                        </option>
                                    </Show>
                                </select>
                            </label>

                            <div class="template-actions">
                                <button
                                    class="secondary-button"
                                    on:click=on_new_template
                                >
                                    "New template"
                                </button>
                                <button
                                    class="secondary-button"
                                    on:click=on_save_template
                                    disabled=move || !has_selection.get()
                                >
                                    "Save template"
                                </button>
                                <button
                                    class="secondary-button delete-template-button"
                                    on:click=on_delete_template
                                    disabled=move || !has_selection.get() || is_new_template.get()
                                >
                                    "Delete"
                                </button>
                            </div>

                            <Show when=move || has_selection.get()>
                                <label class="field">
                                    <span class="field-label">"Name"</span>
                                    <input
                                        type="text"
                                        prop:value=move || editing_name.get()
                                        on:input=move |event| editing_name.set(event_target_value(&event))
                                        placeholder="Template name"
                                    />
                                </label>

                                <label class="field">
                                    <span class="field-label">"Prompt"</span>
                                    <textarea
                                        class="template-prompt-textarea"
                                        prop:value=move || editing_prompt.get()
                                        on:input=move |event| editing_prompt.set(event_target_value(&event))
                                        placeholder="Enter your prompt here. Use {{transcript}} where you want the transcript text inserted."
                                        rows="10"
                                    ></textarea>
                                </label>

                                <p class="field-hint">
                                    "Use "
                                    <code>"{{transcript}}"</code>
                                    " where you want the transcript text inserted."
                                </p>

                                <Show when=move || prompt_missing_placeholder.get()>
                                    <p class="field-hint field-warning">
                                        "This prompt does not contain {{transcript}}. The transcript text will not be included when post-processing runs."
                                    </p>
                                </Show>
                            </Show>

                            <Show when=move || save_feedback.get().is_some()>
                                <p class="feedback">{move || save_feedback.get().unwrap_or_default()}</p>
                            </Show>

                            <Show when=move || !has_templates.get() && !is_new_template.get()>
                                <div class="template-empty-state">
                                    <p class="body-copy">
                                        "No templates found. Create your first template or they will be restored on next app launch."
                                    </p>
                                </div>
                            </Show>

                            <Show when=move || had_result_on_load.get()>
                                <div class="postprocess-previous-chip">
                                    <span class="mini-chip">"Previously processed"</span>
                                    <span class="field-hint">"You can re-run with the same or a different template."</span>
                                </div>
                            </Show>

                            <button
                                class="primary-button postprocess-run-button"
                                on:click=on_run_postprocess
                                disabled=move || !can_run.get()
                            >
                                {move || run_button_label.get()}
                            </button>

                            <Show when=move || !has_transcript.get()>
                                <p class="field-hint">
                                    "No transcript available. Complete a transcription first."
                                </p>
                            </Show>
                        </div>
                    </section>
                </div>

                <div class="postprocess-result-column">
                    <PostprocessTranscriptPreview transcript=transcript_signal />
                    <PostprocessResultPanel
                        result_text=result_text
                        error_message=error_message
                        is_running=is_running
                        copy_feedback=copy_feedback
                    />
                </div>
            </div>
        </section>
    }
}

#[component]
fn PostprocessTranscriptPreview(transcript: Signal<Option<TranscriptResult>>) -> impl IntoView {
    let has_transcript = Signal::derive(move || transcript.get().is_some());

    view! {
        <section class="section">
            <p class="tag">"Source"</p>
            <h3>"Transcript preview"</h3>

            <Show
                when=move || has_transcript.get()
                fallback=|| {
                    view! {
                        <p class="body-copy postprocess-empty-hint">
                            "No transcript available. Complete a transcription on the Transcription screen first."
                        </p>
                    }
                }
            >
                {move || {
                    transcript.get().map(|result| {
                        let provider = result.source.provider.clone();
                        let model_id = result.source.model_id.clone();
                        let duration_label = result
                            .source
                            .duration_ms
                            .map(|ms| format_timestamp(ms as i64))
                            .unwrap_or_else(|| "Unknown".to_string());
                        let text = result.text.clone();

                        view! {
                            <div class="stack">
                                <div class="mini-status">
                                    <span class="mini-chip">{format!("Engine: {provider}")}</span>
                                    <span class="mini-chip">{format!("Model: {model_id}")}</span>
                                    <span class="mini-chip">{format!("Duration: {duration_label}")}</span>
                                </div>
                                <div class="postprocess-transcript-preview">{text}</div>
                            </div>
                        }
                    })
                }}
            </Show>
        </section>
    }
}

#[component]
fn PostprocessResultPanel(
    result_text: RwSignal<Option<String>>,
    error_message: RwSignal<Option<String>>,
    is_running: RwSignal<bool>,
    copy_feedback: RwSignal<Option<&'static str>>,
) -> impl IntoView {
    let has_result = Signal::derive(move || result_text.get().is_some());
    let has_error = Signal::derive(move || error_message.get().is_some());
    let export_feedback = RwSignal::new(None::<&'static str>);

    let copy_button_label = Signal::derive(move || copy_feedback.get().unwrap_or("Copy result"));
    let copy_button_class = Signal::derive(move || match copy_feedback.get() {
        Some("Copied") => "secondary-button success",
        Some("Copy failed") => "secondary-button error",
        _ => "secondary-button",
    });

    let export_button_label =
        Signal::derive(move || export_feedback.get().unwrap_or("Export result"));
    let export_button_class = Signal::derive(move || match export_feedback.get() {
        Some("Exported") => "secondary-button success",
        Some("Export failed") => "secondary-button error",
        _ => "secondary-button",
    });

    Effect::new(move |_| {
        result_text.get();
        export_feedback.set(None);
    });

    let on_copy = move |_: leptos::ev::MouseEvent| {
        let Some(text) = result_text.get_untracked() else {
            return;
        };
        copy_feedback.set(None);

        spawn_local(async move {
            match write_clipboard_text(&text).await {
                Ok(()) => copy_feedback.set(Some("Copied")),
                Err(_) => copy_feedback.set(Some("Copy failed")),
            }
        });
    };

    let on_export = move |_: leptos::ev::MouseEvent| {
        let Some(text) = result_text.get_untracked() else {
            return;
        };
        export_feedback.set(None);

        spawn_local(async move {
            match crate::tauri_api::pick_save_file("postprocess-result.txt").await {
                Ok(Some(path)) => match crate::tauri_api::write_text_file(&path, &text).await {
                    Ok(()) => export_feedback.set(Some("Exported")),
                    Err(_) => export_feedback.set(Some("Export failed")),
                },
                Ok(None) => {}
                Err(_) => export_feedback.set(Some("Export failed")),
            }
        });
    };

    view! {
        <section class="section">
            <p class="tag">"Output"</p>
            <h3>"Post-processed result"</h3>

            <Show when=move || is_running.get()>
                <div class="postprocess-loading">
                    <div class="postprocess-spinner"></div>
                    <p class="body-copy">"Sending transcript to the API for processing..."</p>
                </div>
            </Show>

            <Show when=move || has_error.get()>
                <div class="postprocess-error">
                    <p class="body-copy">{move || error_message.get().unwrap_or_default()}</p>
                </div>
            </Show>

            <Show when=move || has_result.get()>
                <div class="stack">
                    <div class="result-toolbar">
                        <div class="copy-actions">
                            <button
                                class=move || copy_button_class.get()
                                on:click=on_copy
                            >
                                {move || copy_button_label.get()}
                            </button>
                            <button
                                class=move || export_button_class.get()
                                on:click=on_export
                            >
                                {move || export_button_label.get()}
                            </button>
                        </div>
                    </div>
                    <div class="postprocess-output">{move || result_text.get().unwrap_or_default()}</div>
                </div>
            </Show>

            <Show when=move || !has_result.get() && !has_error.get() && !is_running.get()>
                <p class="body-copy postprocess-empty-hint">
                    "Select a template and run post-processing to see results here."
                </p>
            </Show>
        </section>
    }
}

fn generate_template_id() -> String {
    let timestamp = js_sys::Date::now() as u64;
    let random = (js_sys::Math::random() * 0xFFFF_FFFFu64 as f64) as u32;
    format!("tpl-{}-{:x}", timestamp, random)
}
