use leptos::prelude::*;

use crate::tauri_api::{NoteSource, NoteSummary};

fn format_short_date(iso: &str) -> String {
    if iso.len() >= 10 {
        let date_part = &iso[..10];
        let parts: Vec<&str> = date_part.split('-').collect();
        if parts.len() == 3 {
            let month = match parts[1] {
                "01" => "Jan",
                "02" => "Feb",
                "03" => "Mar",
                "04" => "Apr",
                "05" => "May",
                "06" => "Jun",
                "07" => "Jul",
                "08" => "Aug",
                "09" => "Sep",
                "10" => "Oct",
                "11" => "Nov",
                "12" => "Dec",
                _ => parts[1],
            };
            return format!("{} {}, {}", month, parts[2], parts[0]);
        }
    }
    iso.to_string()
}

fn source_label(source: &NoteSource) -> &'static str {
    match source {
        NoteSource::Manual => "Manual",
        NoteSource::Transcription => "Transcription",
        NoteSource::PostProcessing => "Post-processing",
    }
}

#[component]
pub fn NoteListPanel(
    notes: RwSignal<Vec<NoteSummary>>,
    selected_note_id: RwSignal<Option<String>>,
    search_query: RwSignal<String>,
    source_filter: RwSignal<Option<NoteSource>>,
    on_select: impl Fn(String) + Copy + Send + Sync + 'static,
    on_new_note: impl Fn(leptos::ev::MouseEvent) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let filtered_notes = Signal::derive(move || {
        let query = search_query.get().to_lowercase();
        let filter = source_filter.get();
        notes
            .get()
            .into_iter()
            .filter(|note| {
                (query.is_empty() || note.title.to_lowercase().contains(&query))
                    && filter.as_ref().is_none_or(|f| &note.source == f)
            })
            .collect::<Vec<_>>()
    });

    let on_search_input = move |event: leptos::ev::Event| {
        search_query.set(event_target_value(&event));
    };

    let on_filter_change = move |event: leptos::ev::Event| {
        let value = event_target_value(&event);
        let parsed = match value.as_str() {
            "manual" => Some(NoteSource::Manual),
            "transcription" => Some(NoteSource::Transcription),
            "post-processing" => Some(NoteSource::PostProcessing),
            _ => None,
        };
        source_filter.set(parsed);
    };

    view! {
        <div class="notes-list-panel">
            <div class="notes-search-bar">
                <input
                    type="text"
                    placeholder="Search notes..."
                    prop:value=move || search_query.get()
                    on:input=on_search_input
                />
                <select on:change=on_filter_change>
                    <option value="">"All"</option>
                    <option value="manual">"Manual"</option>
                    <option value="transcription">"Transcription"</option>
                    <option value="post-processing">"Post-processing"</option>
                </select>
            </div>

            <button class="secondary-button" on:click=on_new_note>"+ New note"</button>

            <div class="notes-list">
                <For
                    each=move || filtered_notes.get()
                    key=|note| note.id.clone()
                    children=move |note| {
                        let id = note.id.clone();
                        let id_for_click = id.clone();
                        let title = note.title.clone();
                        let date = format_short_date(&note.updated_at);
                        let source = source_label(&note.source).to_string();

                        view! {
                            <div
                                class="notes-list-item"
                                class:notes-list-item-active=move || selected_note_id.get().as_deref() == Some(&*id)
                                on:click=move |_| on_select(id_for_click.clone())
                            >
                                <p class="notes-list-item-title">{title}</p>
                                <div class="notes-list-item-meta">
                                    <span>{date}</span>
                                    <span class="mini-chip">{source}</span>
                                </div>
                            </div>
                        }
                    }
                />
            </div>

            <Show when=move || filtered_notes.get().is_empty()>
                <p class="field-hint">"No notes found."</p>
            </Show>
        </div>
    }
}
