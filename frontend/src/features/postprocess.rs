use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::tauri_api::{list_templates, save_templates, PostProcessTemplate};

#[component]
pub fn PostProcessScreen(active: Signal<bool>) -> impl IntoView {
    let templates = RwSignal::new(Vec::<PostProcessTemplate>::new());
    let selected_template_id = RwSignal::new(None::<String>);
    let editing_name = RwSignal::new(String::new());
    let editing_prompt = RwSignal::new(String::new());
    let is_new_template = RwSignal::new(false);
    let save_feedback = RwSignal::new(None::<String>);

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
                        </div>
                    </section>
                </div>

                <div class="postprocess-result-column">
                    <section class="section postprocess-result-placeholder">
                        <p class="tag">"Output"</p>
                        <h3>"Post-processed result"</h3>
                        <p class="body-copy">
                            "Run post-processing to see results here. Select a template and use the transcript from the Transcription screen."
                        </p>
                    </section>
                </div>
            </div>
        </section>
    }
}

fn generate_template_id() -> String {
    let timestamp = js_sys::Date::now() as u64;
    let random = (js_sys::Math::random() * 0xFFFF_FFFFu64 as f64) as u32;
    format!("tpl-{}-{:x}", timestamp, random)
}
