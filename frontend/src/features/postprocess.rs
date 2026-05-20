use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{
    cancel_postprocess, get_note, get_settings, list_notes, list_templates, run_postprocess,
    save_templates, write_clipboard_text, NoteSummary, PostProcessTemplate,
    PostprocessProviderMode,
};

#[component]
pub fn ProcessScreen(
    active: Signal<bool>,
    pending_process_text: RwSignal<Option<String>>,
) -> impl IntoView {
    let templates = RwSignal::new(Vec::<PostProcessTemplate>::new());
    let selected_template_id = RwSignal::new(None::<String>);
    let editing_name = RwSignal::new(String::new());
    let editing_prompt = RwSignal::new(String::new());
    let is_new_template = RwSignal::new(false);
    let save_feedback = RwSignal::new(None::<String>);

    let input_text = RwSignal::new(String::new());
    let is_running = RwSignal::new(false);
    let result_text = RwSignal::new(None::<String>);
    let error_message = RwSignal::new(None::<String>);
    let copy_feedback = RwSignal::new(None::<&'static str>);
    let postprocess_mode = RwSignal::new(PostprocessProviderMode::Api);
    let enable_thinking = RwSignal::new(false);
    let note_slot_assignments = RwSignal::new(HashMap::<String, String>::new());
    let available_notes = RwSignal::new(Vec::<NoteSummary>::new());

    let detected_slots = Signal::derive(move || {
        let prompt = editing_prompt.get();
        let mut slots = Vec::new();
        let pattern = "{{note";
        let mut search_start = 0;
        while let Some(start) = prompt[search_start..].find(pattern) {
            let abs_start = search_start + start;
            let after_note = abs_start + pattern.len();
            let digit_end = prompt[after_note..]
                .find(|c: char| !c.is_ascii_digit())
                .map(|i| after_note + i)
                .unwrap_or(prompt.len());
            let digits = &prompt[after_note..digit_end];
            if !digits.is_empty() && prompt[digit_end..].starts_with("}}") {
                let slot_name = format!("note{digits}");
                if !slots.contains(&slot_name) {
                    slots.push(slot_name);
                }
                search_start = digit_end + 2;
            } else {
                search_start = after_note;
            }
        }
        slots.sort();
        slots
    });

    let apply_selection = move |id: Option<String>, source: &[PostProcessTemplate]| {
        selected_template_id.set(id.clone());
        is_new_template.set(false);
        note_slot_assignments.set(HashMap::new());
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
            if let Ok(settings) = get_settings().await {
                postprocess_mode.set(settings.postprocess_provider_mode);
            }

            if let Ok(notes) = list_notes().await {
                available_notes.set(notes);
            }

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
        note_slot_assignments.set(HashMap::new());
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
        !prompt.is_empty() && !prompt.contains("{{transcript}}") && detected_slots.get().is_empty()
    });

    let has_templates = Signal::derive(move || !templates.get().is_empty());
    let has_selection = Signal::derive(move || selected_template_id.get().is_some());

    let has_input = Signal::derive(move || !input_text.get().trim().is_empty());
    let template_uses_transcript =
        Signal::derive(move || editing_prompt.get().contains("{{transcript}}"));

    let slots_all_assigned = Signal::derive(move || {
        let slots = detected_slots.get();
        let assignments = note_slot_assignments.get();
        slots.iter().all(|s| assignments.contains_key(s))
    });

    let can_run = Signal::derive(move || {
        has_selection.get()
            && !is_running.get()
            && slots_all_assigned.get()
            && (!template_uses_transcript.get() || has_input.get())
    });

    let run_button_label = Signal::derive(move || {
        if is_running.get() {
            match postprocess_mode.get() {
                PostprocessProviderMode::LocalLlm => "Processing with local LLM...",
                PostprocessProviderMode::Api => "Sending to API...",
            }
        } else {
            "Run"
        }
    });

    let on_run_postprocess = move |_| {
        let Some(template_id) = selected_template_id.get_untracked() else {
            return;
        };
        let text = input_text.get_untracked();
        if template_uses_transcript.get_untracked() && text.trim().is_empty() {
            return;
        }

        is_running.set(true);
        error_message.set(None);
        result_text.set(None);
        copy_feedback.set(None);

        let assignments = note_slot_assignments.get_untracked();

        spawn_local(async move {
            let thinking = enable_thinking.get_untracked();
            match run_postprocess(text, template_id, thinking, assignments).await {
                Ok(processed) => {
                    result_text.set(Some(processed));
                }
                Err(err) => {
                    if err.contains("cancelled") {
                        // Treat cancellation as neutral — not an error
                    } else {
                        error_message.set(Some(err));
                    }
                }
            }
            is_running.set(false);
        });
    };

    // When activated with pending text from another screen, fill input and clear the signal
    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        if let Some(text) = pending_process_text.get_untracked() {
            input_text.set(text);
            pending_process_text.set(None);
        }
    });

    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Process"</h2>
                <p>"Apply LLM templates to any text \u{2014} transcripts, notes, or anything you paste."</p>
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
                                    " where you want the transcript text inserted. Use "
                                    <code>"{{note1}}"</code>
                                    ", "
                                    <code>"{{note2}}"</code>
                                    ", etc. to create note slots."
                                </p>

                                <Show when=move || prompt_missing_placeholder.get()>
                                    <p class="field-hint field-warning">
                                        "This prompt does not contain {{transcript}} or any note slots."
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

                            <Show when=move || matches!(postprocess_mode.get(), PostprocessProviderMode::LocalLlm)>
                                <label class="checkbox-row">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || enable_thinking.get()
                                        on:change=move |ev| enable_thinking.set(event_target_checked(&ev))
                                    />
                                    <span>"Enable thinking mode"</span>
                                </label>
                                <p class="field-hint">
                                    "Lets the model reason before answering. Best with larger models (4B+). May slow down small models."
                                </p>
                            </Show>

                            <button
                                class="primary-button postprocess-run-button"
                                on:click=on_run_postprocess
                                disabled=move || !can_run.get()
                            >
                                {move || run_button_label.get()}
                            </button>

                        </div>
                    </section>
                </div>

                <div class="postprocess-result-column">
                    <section class="section">
                        <p class="tag">"Input"</p>
                        <h3>"Text input"</h3>

                        <div class="stack">
                            <div class="load-from-note-row">
                                <select
                                    class="load-from-note-select"
                                    on:change=move |event| {
                                        let note_id = event_target_value(&event);
                                        if note_id.is_empty() {
                                            return;
                                        }
                                        // Reset select back to placeholder
                                        if let Some(target) = event.target() {
                                            use wasm_bindgen::JsCast;
                                            if let Ok(select) = target.dyn_into::<web_sys::HtmlSelectElement>() {
                                                select.set_selected_index(0);
                                            }
                                        }
                                        spawn_local(async move {
                                            if let Ok(Some(note)) = get_note(&note_id).await {
                                                input_text.set(note.content);
                                            }
                                        });
                                    }
                                >
                                    <option value="" disabled=true selected=true>
                                        {move || {
                                            if available_notes.get().is_empty() {
                                                "No notes saved yet"
                                            } else {
                                                "Load from note..."
                                            }
                                        }}
                                    </option>
                                    <For
                                        each=move || available_notes.get()
                                        key=|note| note.id.clone()
                                        children=move |note| {
                                            view! {
                                                <option value=note.id.clone()>{note.title.clone()}</option>
                                            }
                                        }
                                    />
                                </select>
                            </div>

                            <Show
                                when=move || template_uses_transcript.get()
                                fallback=move || {
                                    view! {
                                        <p class="field-hint">
                                            "This template doesn\u{2019}t use "
                                            <code>"{{transcript}}"</code>
                                            ". You can add it to your prompt, or just use note slots below."
                                        </p>
                                    }
                                }
                            >
                                <textarea
                                    class="process-input-textarea"
                                    prop:value=move || input_text.get()
                                    on:input=move |event| input_text.set(event_target_value(&event))
                                    placeholder="Paste or type text here, or load content from a saved note."
                                    rows="10"
                                ></textarea>
                            </Show>
                        </div>
                    </section>

                    <Show when=move || !detected_slots.get().is_empty()>
                        <section class="section">
                            <div class="note-slot-assignments">
                                <span class="field-label">"Note assignments"</span>
                                <For
                                    each=move || detected_slots.get()
                                    key=|slot| slot.clone()
                                    children=move |slot| {
                                        let slot_for_label = slot.clone();
                                        let slot_for_change = slot.clone();
                                        let current_value = Signal::derive({
                                            let slot = slot.clone();
                                            move || {
                                                note_slot_assignments
                                                    .get()
                                                    .get(&slot)
                                                    .cloned()
                                                    .unwrap_or_default()
                                            }
                                        });
                                        view! {
                                            <label class="field">
                                                <span class="field-label">{slot_for_label}</span>
                                                <select
                                                    prop:value=move || current_value.get()
                                                    on:change=move |event| {
                                                        let value = event_target_value(&event);
                                                        let slot = slot_for_change.clone();
                                                        note_slot_assignments.update(|map| {
                                                            if value.is_empty() {
                                                                map.remove(&slot);
                                                            } else {
                                                                map.insert(slot, value);
                                                            }
                                                        });
                                                    }
                                                >
                                                    <option value="" disabled=true selected=move || current_value.get().is_empty()>
                                                        {move || {
                                                            if available_notes.get().is_empty() {
                                                                "No notes available"
                                                            } else {
                                                                "Select a note..."
                                                            }
                                                        }}
                                                    </option>
                                                    <For
                                                        each=move || available_notes.get()
                                                        key=|note| note.id.clone()
                                                        children=move |note| {
                                                            view! {
                                                                <option value=note.id.clone()>{note.title.clone()}</option>
                                                            }
                                                        }
                                                    />
                                                </select>
                                            </label>
                                        }
                                    }
                                />
                                <Show when=move || !slots_all_assigned.get()>
                                    <p class="field-hint field-warning">
                                        "Assign a note to each slot before running."
                                    </p>
                                </Show>
                            </div>
                        </section>
                    </Show>

                    <PostprocessResultPanel
                        result_text=result_text
                        error_message=error_message
                        is_running=is_running
                        copy_feedback=copy_feedback
                        postprocess_mode=postprocess_mode
                        editing_name=editing_name
                    />
                </div>
            </div>
        </section>
    }
}

#[component]
fn PostprocessResultPanel(
    result_text: RwSignal<Option<String>>,
    error_message: RwSignal<Option<String>>,
    is_running: RwSignal<bool>,
    copy_feedback: RwSignal<Option<&'static str>>,
    postprocess_mode: RwSignal<PostprocessProviderMode>,
    editing_name: RwSignal<String>,
) -> impl IntoView {
    let has_result = Signal::derive(move || result_text.get().is_some());
    let has_error = Signal::derive(move || error_message.get().is_some());
    let export_feedback = RwSignal::new(None::<&'static str>);
    let save_note_feedback = RwSignal::new(None::<&'static str>);

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
        save_note_feedback.set(None);
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

    let on_save_note = move |_: leptos::ev::MouseEvent| {
        let Some(text) = result_text.get_untracked() else {
            return;
        };
        save_note_feedback.set(None);

        let template_name = editing_name.get_untracked();

        spawn_local(async move {
            let date = js_sys::Date::new_0();
            let date_str = format!(
                "{}-{:02}-{:02}",
                date.get_full_year(),
                date.get_month() + 1,
                date.get_date(),
            );
            let title = format!("{} - {}", template_name, date_str);

            match crate::tauri_api::create_note(
                title,
                text,
                crate::tauri_api::NoteSource::PostProcessing,
            )
            .await
            {
                Ok(_) => save_note_feedback.set(Some("Saved")),
                Err(_) => save_note_feedback.set(Some("Save failed")),
            }
        });
    };

    let save_note_button_label =
        Signal::derive(move || save_note_feedback.get().unwrap_or("Save as note"));
    let save_note_button_class = Signal::derive(move || match save_note_feedback.get() {
        Some("Saved") => "secondary-button success",
        Some("Save failed") => "secondary-button error",
        _ => "secondary-button",
    });

    view! {
        <section class="section">
            <p class="tag">"Output"</p>
            <h3>"Result"</h3>

            <Show when=move || is_running.get()>
                <div class="postprocess-loading">
                    <div class="postprocess-spinner"></div>
                    <p class="body-copy">
                        {move || match postprocess_mode.get() {
                            PostprocessProviderMode::LocalLlm => "Processing with local LLM...",
                            PostprocessProviderMode::Api => "Sending to API...",
                        }}
                    </p>
                    <Show when=move || matches!(postprocess_mode.get(), PostprocessProviderMode::LocalLlm)>
                        <button
                            class="secondary-button"
                            on:click=move |_| {
                                spawn_local(async move {
                                    let _ = cancel_postprocess().await;
                                });
                            }
                        >
                            "Cancel"
                        </button>
                    </Show>
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
                            <button
                                class=move || save_note_button_class.get()
                                on:click=on_save_note
                            >
                                {move || save_note_button_label.get()}
                            </button>
                        </div>
                    </div>
                    <div class="postprocess-output">{move || result_text.get().unwrap_or_default()}</div>
                </div>
            </Show>

            <Show when=move || !has_result.get() && !has_error.get() && !is_running.get()>
                <p class="body-copy postprocess-empty-hint">
                    "Select a template and click Run to see results here."
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
