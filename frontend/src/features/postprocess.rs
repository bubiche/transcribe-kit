use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{
    cancel_postprocess, get_note, get_settings, list_notes, list_templates, run_postprocess,
    run_postprocess_follow_up, save_templates, write_clipboard_text, ChatMessage, NoteSummary,
    PostProcessTemplate, PostprocessProviderMode,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurn {
    pub id: u64,
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatRole {
    User,
    Assistant,
}

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
    let initial_prompt = RwSignal::new(None::<String>);
    let turns = RwSignal::new(Vec::<ChatTurn>::new());
    let next_turn_id = RwSignal::new(0u64);
    let thread_template_name = RwSignal::new(None::<String>);
    let follow_up_input = RwSignal::new(String::new());

    let next_id = move || {
        let id = next_turn_id.get_untracked();
        next_turn_id.set(id + 1);
        id
    };
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
        initial_prompt.set(None);
        turns.set(Vec::new());
        next_turn_id.set(0);
        thread_template_name.set(None);
        follow_up_input.set(String::new());
        copy_feedback.set(None);

        let assignments = note_slot_assignments.get_untracked();
        let template_name_snapshot = editing_name.get_untracked();

        spawn_local(async move {
            let thinking = enable_thinking.get_untracked();
            match run_postprocess(text, template_id, thinking, assignments).await {
                Ok(result) => {
                    initial_prompt.set(Some(result.rendered_prompt));
                    turns.set(vec![ChatTurn {
                        id: next_id(),
                        role: ChatRole::Assistant,
                        content: result.response,
                    }]);
                    thread_template_name.set(Some(template_name_snapshot));
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

    let on_send_follow_up = move || {
        if is_running.get_untracked() {
            return;
        }
        let text = follow_up_input.get_untracked();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let Some(prompt) = initial_prompt.get_untracked() else {
            return;
        };

        let user_turn = ChatTurn {
            id: next_id(),
            role: ChatRole::User,
            content: trimmed.to_string(),
        };

        turns.update(|t| t.push(user_turn.clone()));
        follow_up_input.set(String::new());
        error_message.set(None);
        copy_feedback.set(None);
        is_running.set(true);

        let dto_turns: Vec<ChatMessage> = turns
            .get_untracked()
            .iter()
            .map(|t| ChatMessage {
                role: match t.role {
                    ChatRole::User => "user".to_string(),
                    ChatRole::Assistant => "assistant".to_string(),
                },
                content: t.content.clone(),
            })
            .collect();

        spawn_local(async move {
            let thinking = enable_thinking.get_untracked();
            match run_postprocess_follow_up(prompt, dto_turns, thinking).await {
                Ok(response) => {
                    turns.update(|t| {
                        t.push(ChatTurn {
                            id: next_id(),
                            role: ChatRole::Assistant,
                            content: response,
                        })
                    });
                }
                Err(err) => {
                    // Pop the optimistic user turn so it doesn't sit there orphaned.
                    let mut current = turns.get_untracked();
                    let popped = current.pop();
                    turns.set(current);
                    let restored = popped.map(|t| t.content).unwrap_or_default();
                    follow_up_input.set(restored);
                    if !err.contains("cancelled") {
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

                    <PostprocessThreadPanel
                        turns=turns
                        thread_template_name=thread_template_name
                        follow_up_input=follow_up_input
                        error_message=error_message
                        is_running=is_running
                        copy_feedback=copy_feedback
                        postprocess_mode=postprocess_mode
                        on_send_follow_up=on_send_follow_up
                    />
                </div>
            </div>
        </section>
    }
}

#[component]
fn PostprocessThreadPanel<F>(
    turns: RwSignal<Vec<ChatTurn>>,
    thread_template_name: RwSignal<Option<String>>,
    follow_up_input: RwSignal<String>,
    error_message: RwSignal<Option<String>>,
    is_running: RwSignal<bool>,
    copy_feedback: RwSignal<Option<&'static str>>,
    postprocess_mode: RwSignal<PostprocessProviderMode>,
    on_send_follow_up: F,
) -> impl IntoView
where
    F: Fn() + Copy + Send + Sync + 'static,
{
    let has_thread = Signal::derive(move || !turns.get().is_empty());
    let has_follow_ups = Signal::derive(move || turns.get().len() > 1);
    let has_error = Signal::derive(move || error_message.get().is_some());
    let export_feedback = RwSignal::new(None::<&'static str>);
    let save_last_feedback = RwSignal::new(None::<&'static str>);
    let save_thread_feedback = RwSignal::new(None::<&'static str>);

    // Reset transient feedback when the thread changes (new run, new turn).
    Effect::new(move |_| {
        turns.get();
        export_feedback.set(None);
        save_last_feedback.set(None);
        save_thread_feedback.set(None);
    });

    let last_assistant_text = Signal::derive(move || {
        turns
            .get()
            .iter()
            .rev()
            .find(|t| t.role == ChatRole::Assistant)
            .map(|t| t.content.clone())
    });

    let copy_button_label = Signal::derive(move || copy_feedback.get().unwrap_or("Copy reply"));
    let copy_button_class = Signal::derive(move || match copy_feedback.get() {
        Some("Copied") => "secondary-button success",
        Some("Copy failed") => "secondary-button error",
        _ => "secondary-button",
    });

    let export_button_label =
        Signal::derive(move || export_feedback.get().unwrap_or("Export reply"));
    let export_button_class = Signal::derive(move || match export_feedback.get() {
        Some("Exported") => "secondary-button success",
        Some("Export failed") => "secondary-button error",
        _ => "secondary-button",
    });

    let on_copy = move |_: leptos::ev::MouseEvent| {
        let Some(text) = last_assistant_text.get_untracked() else {
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
        let Some(text) = last_assistant_text.get_untracked() else {
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

    let on_save_last = move |_: leptos::ev::MouseEvent| {
        let Some(text) = last_assistant_text.get_untracked() else {
            return;
        };
        save_last_feedback.set(None);
        let template_name = thread_template_name.get_untracked().unwrap_or_default();
        spawn_local(async move {
            let date_str = today_date_string();
            let title = format!("{} - {}", template_name, date_str);
            match crate::tauri_api::create_note(
                title,
                text,
                crate::tauri_api::NoteSource::PostProcessing,
            )
            .await
            {
                Ok(_) => save_last_feedback.set(Some("Saved")),
                Err(_) => save_last_feedback.set(Some("Save failed")),
            }
        });
    };

    let on_save_thread = move |_: leptos::ev::MouseEvent| {
        let snapshot = turns.get_untracked();
        if snapshot.is_empty() {
            return;
        }
        save_thread_feedback.set(None);
        let template_name = thread_template_name.get_untracked().unwrap_or_default();
        let body = render_thread_markdown(&template_name, &snapshot);
        spawn_local(async move {
            let date_str = today_date_string();
            let title = format!("{} (chat) - {}", template_name, date_str);
            match crate::tauri_api::create_note(
                title,
                body,
                crate::tauri_api::NoteSource::PostProcessing,
            )
            .await
            {
                Ok(_) => save_thread_feedback.set(Some("Saved")),
                Err(_) => save_thread_feedback.set(Some("Save failed")),
            }
        });
    };

    let save_last_label = Signal::derive(move || {
        let default_label = if has_follow_ups.get() {
            "Save last message"
        } else {
            "Save as note"
        };
        save_last_feedback.get().unwrap_or(default_label)
    });
    let save_last_class = Signal::derive(move || match save_last_feedback.get() {
        Some("Saved") => "secondary-button success",
        Some("Save failed") => "secondary-button error",
        _ => "secondary-button",
    });

    let save_thread_label =
        Signal::derive(move || save_thread_feedback.get().unwrap_or("Save full thread"));
    let save_thread_class = Signal::derive(move || match save_thread_feedback.get() {
        Some("Saved") => "secondary-button success",
        Some("Save failed") => "secondary-button error",
        _ => "secondary-button",
    });

    let send_disabled =
        Signal::derive(move || is_running.get() || follow_up_input.get().trim().is_empty());

    let on_follow_up_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.meta_key() || ev.ctrl_key()) {
            ev.prevent_default();
            on_send_follow_up();
        }
    };

    let on_send_click = move |_: leptos::ev::MouseEvent| on_send_follow_up();

    view! {
        <section class="section">
            <p class="tag">"Output"</p>
            <h3>"Result"</h3>

            <Show when=move || has_thread.get()>
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
                                class=move || save_last_class.get()
                                on:click=on_save_last
                            >
                                {move || save_last_label.get()}
                            </button>
                            <Show when=move || has_follow_ups.get()>
                                <button
                                    class=move || save_thread_class.get()
                                    on:click=on_save_thread
                                >
                                    {move || save_thread_label.get()}
                                </button>
                            </Show>
                        </div>
                    </div>

                    <div class="postprocess-thread">
                        <For
                            each=move || turns.get()
                            key=|turn| turn.id
                            children=move |turn| {
                                let role_label = match turn.role {
                                    ChatRole::User => "You",
                                    ChatRole::Assistant => "Assistant",
                                };
                                let turn_class = match turn.role {
                                    ChatRole::User => "postprocess-turn postprocess-turn-user",
                                    ChatRole::Assistant => "postprocess-turn",
                                };
                                view! {
                                    <article class=turn_class>
                                        <p class="postprocess-turn-role">{role_label}</p>
                                        <div class="postprocess-turn-body">{turn.content}</div>
                                    </article>
                                }
                            }
                        />
                    </div>
                </div>
            </Show>

            <Show when=move || has_error.get()>
                <div class="postprocess-error">
                    <p class="body-copy">{move || error_message.get().unwrap_or_default()}</p>
                </div>
            </Show>

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

            <Show when=move || has_thread.get() && !is_running.get()>
                <div class="postprocess-follow-up">
                    <textarea
                        class="postprocess-follow-up-textarea"
                        prop:value=move || follow_up_input.get()
                        on:input=move |event| follow_up_input.set(event_target_value(&event))
                        on:keydown=on_follow_up_keydown
                        placeholder="Follow up… (⌘/Ctrl+Enter to send)"
                        rows="3"
                    ></textarea>
                    <div class="postprocess-follow-up-actions">
                        <p class="field-hint postprocess-follow-up-hint">
                            "Press ⌘/Ctrl+Enter to send."
                        </p>
                        <button
                            class="primary-button"
                            on:click=on_send_click
                            disabled=move || send_disabled.get()
                        >
                            "Send"
                        </button>
                    </div>
                </div>
            </Show>

            <Show when=move || !has_thread.get() && !has_error.get() && !is_running.get()>
                <p class="body-copy postprocess-empty-hint">
                    "Select a template and click Run to see results here."
                </p>
            </Show>
        </section>
    }
}

fn today_date_string() -> String {
    let date = js_sys::Date::new_0();
    format!(
        "{}-{:02}-{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date(),
    )
}

fn render_thread_markdown(template_name: &str, turns: &[ChatTurn]) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(template_name);
    out.push_str("\n\n");
    for turn in turns {
        let role = match turn.role {
            ChatRole::User => "## You",
            ChatRole::Assistant => "## Assistant",
        };
        out.push_str(role);
        out.push_str("\n\n");
        out.push_str(turn.content.trim_end());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

fn generate_template_id() -> String {
    let timestamp = js_sys::Date::now() as u64;
    let random = (js_sys::Math::random() * 0xFFFF_FFFFu64 as f64) as u32;
    format!("tpl-{}-{:x}", timestamp, random)
}
