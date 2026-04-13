use leptos::prelude::*;

use crate::tauri_api::Note;

#[component]
pub fn NoteEditorPanel(
    note: RwSignal<Option<Note>>,
    is_new: RwSignal<bool>,
    on_save: impl Fn((String, String)) + Copy + Send + Sync + 'static,
    on_delete: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let editing_title = RwSignal::new(String::new());
    let editing_content = RwSignal::new(String::new());

    // Sync editing state from note prop
    Effect::new(move |_| {
        if let Some(note) = note.get() {
            editing_title.set(note.title);
            editing_content.set(note.content);
        } else {
            editing_title.set(String::new());
            editing_content.set(String::new());
        }
    });

    let handle_save = move |_| {
        let title = editing_title.get_untracked();
        let content = editing_content.get_untracked();
        on_save((title, content));
    };

    let handle_delete = move |_| {
        if let Some(note) = note.get_untracked() {
            on_delete(note.id);
        }
    };

    view! {
        <div class="notes-editor-panel">
            <Show when=move || note.get().is_some() || is_new.get()>
                <label class="field">
                    <span class="field-label">"Title"</span>
                    <input
                        type="text"
                        placeholder="Note title"
                        prop:value=move || editing_title.get()
                        on:input=move |event| editing_title.set(event_target_value(&event))
                    />
                </label>

                <label class="field">
                    <span class="field-label">"Content"</span>
                    <textarea
                        class="notes-content-textarea"
                        placeholder="Write your note here..."
                        prop:value=move || editing_content.get()
                        on:input=move |event| editing_content.set(event_target_value(&event))
                    ></textarea>
                </label>

                <div class="notes-editor-actions">
                    <button
                        class="secondary-button delete-note-button"
                        on:click=handle_delete
                        disabled=move || is_new.get()
                    >
                        "Delete"
                    </button>
                    <button
                        class="primary-button"
                        on:click=handle_save
                        disabled=move || editing_title.get().trim().is_empty()
                    >
                        {move || if is_new.get() { "Create note" } else { "Save" }}
                    </button>
                </div>
            </Show>

            <Show when=move || note.get().is_none() && !is_new.get()>
                <div class="notes-editor-empty">
                    <p class="body-copy">"Select a note from the list, or create a new one."</p>
                </div>
            </Show>
        </div>
    }
}
