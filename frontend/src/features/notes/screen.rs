use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::tauri_api::{
    create_note, delete_note, get_note, list_notes, update_note, Note, NoteSource, NoteSummary,
};

use super::editor::NoteEditorPanel;
use super::list::NoteListPanel;

#[component]
pub fn NotesScreen(active: Signal<bool>) -> impl IntoView {
    let notes = RwSignal::new(Vec::<NoteSummary>::new());
    let selected_note_id = RwSignal::new(None::<String>);
    let selected_note = RwSignal::new(None::<Note>);
    let search_query = RwSignal::new(String::new());
    let source_filter = RwSignal::new(None::<NoteSource>);
    let refresh_nonce = RwSignal::new(0_u64);
    let is_new_note = RwSignal::new(false);

    // Resizable list panel
    let grid_ref = NodeRef::<Div>::new();
    let list_width_px = RwSignal::new(None::<f64>);
    let is_resizing = RwSignal::new(false);

    let grid_style = move || match list_width_px.get() {
        Some(w) => format!("grid-template-columns: {w}px 0px 1fr"),
        None => String::new(),
    };

    let on_handle_pointerdown = move |event: web_sys::PointerEvent| {
        event.prevent_default();
        if let Some(target) = event.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(event.pointer_id());
            }
        }
        is_resizing.set(true);
    };

    let on_handle_pointermove = move |event: web_sys::PointerEvent| {
        if !is_resizing.get_untracked() {
            return;
        }
        if let Some(grid) = grid_ref.get() {
            let el: &web_sys::Element = &grid;
            let rect = el.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let max_w = rect.width() * 0.6;
            list_width_px.set(Some(x.max(180.0).min(max_w)));
        }
    };

    let on_handle_pointerup = move |_: web_sys::PointerEvent| {
        is_resizing.set(false);
    };

    // Effect 1: reload note list when active or refresh_nonce changes
    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        refresh_nonce.get();

        spawn_local(async move {
            match list_notes().await {
                Ok(loaded) => notes.set(loaded),
                Err(_) => notes.set(Vec::new()),
            }
        });
    });

    // Effect 2: load full note when selected_note_id changes
    Effect::new(move |_| {
        let Some(id) = selected_note_id.get() else {
            selected_note.set(None);
            return;
        };
        if is_new_note.get_untracked() {
            return;
        }

        spawn_local(async move {
            match get_note(&id).await {
                Ok(note) => selected_note.set(note),
                Err(_) => selected_note.set(None),
            }
        });
    });

    let on_new_note = move |_| {
        is_new_note.set(true);
        selected_note_id.set(None);
        selected_note.set(None);
    };

    let on_select = move |id: String| {
        is_new_note.set(false);
        selected_note_id.set(Some(id));
    };

    let on_save = move |(title, content): (String, String)| {
        let new = is_new_note.get_untracked();
        let current_id = selected_note_id.get_untracked();

        spawn_local(async move {
            let result = if new {
                create_note(title, content, NoteSource::Manual).await
            } else if let Some(id) = current_id {
                update_note(id, title, content).await
            } else {
                return;
            };

            if let Ok(saved) = result {
                is_new_note.set(false);
                selected_note_id.set(Some(saved.id));
                refresh_nonce.update(|n| *n = n.saturating_add(1));
            }
        });
    };

    let on_delete = move |id: String| {
        spawn_local(async move {
            if delete_note(&id).await.is_ok() {
                selected_note_id.set(None);
                selected_note.set(None);
                is_new_note.set(false);
                refresh_nonce.update(|n| *n = n.saturating_add(1));
            }
        });
    };

    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Notes"</h2>
                <p>"Saved transcriptions, post-processing results, and your own notes."</p>
            </div>
            <div
                class="notes-grid"
                class:notes-grid-resizing=move || is_resizing.get()
                node_ref=grid_ref
                style=grid_style
            >
                <NoteListPanel
                    notes=notes
                    selected_note_id=selected_note_id
                    search_query=search_query
                    source_filter=source_filter
                    on_select=on_select
                    on_new_note=on_new_note
                />
                <div
                    class="notes-resize-handle"
                    on:pointerdown=on_handle_pointerdown
                    on:pointermove=on_handle_pointermove
                    on:pointerup=on_handle_pointerup
                ></div>
                <NoteEditorPanel
                    note=selected_note
                    is_new=is_new_note
                    on_save=on_save
                    on_delete=on_delete
                />
            </div>
        </section>
    }
}
