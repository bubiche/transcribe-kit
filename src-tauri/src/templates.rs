use std::{fs, path::PathBuf};

use directories::ProjectDirs;

use std::collections::HashMap;

use crate::{models::PostProcessTemplate, settings::SettingsError};

#[derive(Debug, Clone)]
pub struct TemplateStore {
    file_path: PathBuf,
}

impl TemplateStore {
    pub fn new() -> Result<Self, SettingsError> {
        let project_dirs = ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit")
            .ok_or(SettingsError::MissingConfigDir)?;

        Ok(Self {
            file_path: project_dirs.config_dir().join("templates.json"),
        })
    }

    #[cfg(test)]
    fn with_path(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    pub fn load(&self) -> Vec<PostProcessTemplate> {
        match fs::read_to_string(&self.file_path) {
            Ok(contents) => match serde_json::from_str::<Vec<PostProcessTemplate>>(&contents) {
                Ok(templates) => templates,
                Err(error) => {
                    eprintln!("Warning: templates.json is malformed, using defaults: {error}");
                    default_templates()
                }
            },
            Err(_) => default_templates(),
        }
    }

    pub fn save(&self, templates: &[PostProcessTemplate]) -> Result<(), SettingsError> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(SettingsError::CreateDirectory)?;
        }

        let contents =
            serde_json::to_string_pretty(templates).expect("template serialization is valid");
        fs::write(&self.file_path, contents).map_err(SettingsError::WriteFile)
    }
}

/// Find a template by ID, returning a clone if found.
pub fn find_template_by_id<'a>(
    templates: &'a [PostProcessTemplate],
    template_id: &str,
) -> Option<&'a PostProcessTemplate> {
    templates.iter().find(|t| t.id == template_id)
}

/// Extract sorted, deduplicated note slot names from a prompt.
///
/// Scans for `{{note` followed by one or more digits then `}}`.
/// Returns e.g. `["note1", "note2"]`.
pub fn extract_note_slots(prompt: &str) -> Vec<String> {
    let pattern = "{{note";
    let mut slots = Vec::new();
    let mut search_start = 0;

    while let Some(start) = prompt[search_start..].find(pattern) {
        let abs_start = search_start + start;
        let after_note = abs_start + pattern.len();

        // Read digits immediately after "{{note"
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
}

/// Replace `{{transcript}}` and `{{noteN}}` placeholders in a prompt.
///
/// `note_contents` maps slot name (e.g. "note1") to the note's text content.
/// Returns `Err` listing any unresolved `{{noteN}}` placeholders still present.
pub fn render_template(
    prompt: &str,
    transcript_text: &str,
    note_contents: &HashMap<String, String>,
) -> Result<String, String> {
    let mut rendered = prompt.replace("{{transcript}}", transcript_text);

    for (slot_name, content) in note_contents {
        let placeholder = format!("{{{{{slot_name}}}}}");
        rendered = rendered.replace(&placeholder, content);
    }

    // Check for any unresolved {{noteN}} placeholders
    let unresolved = extract_note_slots(&rendered);
    if !unresolved.is_empty() {
        return Err(format!(
            "Unresolved note slots: {}",
            unresolved
                .iter()
                .map(|s| format!("{{{{{s}}}}}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(rendered)
}

pub fn default_templates() -> Vec<PostProcessTemplate> {
    vec![
        PostProcessTemplate {
            id: "builtin-cleanup".to_string(),
            name: "Clean up transcript".to_string(),
            prompt: "Clean up the following transcript. Fix grammar, remove filler words, and improve readability while preserving the original meaning.\n\n{{transcript}}".to_string(),
        },
        PostProcessTemplate {
            id: "builtin-meeting-notes".to_string(),
            name: "Meeting notes".to_string(),
            prompt: "Convert the following transcript into structured meeting notes with key points, action items, and decisions.\n\n{{transcript}}".to_string(),
        },
        PostProcessTemplate {
            id: "builtin-summary".to_string(),
            name: "Summary".to_string(),
            prompt: "Write a concise summary of the following transcript.\n\n{{transcript}}".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn temp_store() -> (TempDir, TemplateStore) {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = TemplateStore::with_path(temp_dir.path().join("templates.json"));
        (temp_dir, store)
    }

    #[test]
    fn load_returns_defaults_when_file_missing() {
        let (_temp_dir, store) = temp_store();

        let templates = store.load();

        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0].id, "builtin-cleanup");
        assert_eq!(templates[1].id, "builtin-meeting-notes");
        assert_eq!(templates[2].id, "builtin-summary");
    }

    #[test]
    fn load_returns_defaults_when_file_is_invalid_json() {
        let (temp_dir, store) = temp_store();

        fs::write(temp_dir.path().join("templates.json"), "not valid json {{{")
            .expect("write corrupt file");

        let templates = store.load();

        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0].id, "builtin-cleanup");
    }

    #[test]
    fn round_trip_save_then_load() {
        let (_temp_dir, store) = temp_store();

        let custom = vec![PostProcessTemplate {
            id: "custom-1".to_string(),
            name: "My template".to_string(),
            prompt: "Do something with {{transcript}}".to_string(),
        }];

        store.save(&custom).expect("save templates");
        let loaded = store.load();

        assert_eq!(loaded, custom);
    }

    #[test]
    fn save_creates_parent_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let nested_path = temp_dir
            .path()
            .join("nested")
            .join("deep")
            .join("templates.json");
        let store = TemplateStore::with_path(nested_path.clone());

        store.save(&default_templates()).expect("save templates");

        assert!(nested_path.exists());
    }

    #[test]
    fn default_templates_all_contain_placeholder() {
        for template in default_templates() {
            assert!(
                template.prompt.contains("{{transcript}}"),
                "template '{}' is missing {{{{transcript}}}} placeholder",
                template.name
            );
        }
    }

    #[test]
    fn load_returns_empty_vec_when_file_contains_empty_array() {
        let (temp_dir, store) = temp_store();

        fs::write(temp_dir.path().join("templates.json"), "[]").expect("write empty array");

        let templates = store.load();

        assert!(templates.is_empty());
    }

    #[test]
    fn postprocess_model_defaults_to_gpt4o_mini() {
        use crate::models::AppSettings;
        let defaults = AppSettings::default();
        assert_eq!(defaults.postprocess_model, "gpt-4o-mini");
    }

    #[test]
    fn save_overwrites_existing_templates() {
        let (_temp_dir, store) = temp_store();

        let first = vec![PostProcessTemplate {
            id: "first".to_string(),
            name: "First".to_string(),
            prompt: "{{transcript}}".to_string(),
        }];
        store.save(&first).expect("save first");

        let second = vec![PostProcessTemplate {
            id: "second".to_string(),
            name: "Second".to_string(),
            prompt: "Do something with {{transcript}}".to_string(),
        }];
        store.save(&second).expect("save second");

        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "second");
    }

    #[test]
    fn load_preserves_template_order() {
        let (_temp_dir, store) = temp_store();

        let templates = vec![
            PostProcessTemplate {
                id: "c".to_string(),
                name: "Charlie".to_string(),
                prompt: "{{transcript}}".to_string(),
            },
            PostProcessTemplate {
                id: "a".to_string(),
                name: "Alpha".to_string(),
                prompt: "{{transcript}}".to_string(),
            },
            PostProcessTemplate {
                id: "b".to_string(),
                name: "Bravo".to_string(),
                prompt: "{{transcript}}".to_string(),
            },
        ];
        store.save(&templates).expect("save");

        let loaded = store.load();
        assert_eq!(loaded[0].id, "c");
        assert_eq!(loaded[1].id, "a");
        assert_eq!(loaded[2].id, "b");
    }

    // ---- find_template_by_id ----

    #[test]
    fn find_template_by_id_returns_matching_template() {
        let templates = default_templates();
        let found = find_template_by_id(&templates, "builtin-cleanup");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Clean up transcript");
    }

    #[test]
    fn find_template_by_id_returns_none_for_missing_id() {
        let templates = default_templates();
        let found = find_template_by_id(&templates, "nonexistent-id");
        assert!(found.is_none());
    }

    #[test]
    fn find_template_by_id_returns_none_for_empty_list() {
        let templates: Vec<PostProcessTemplate> = vec![];
        let found = find_template_by_id(&templates, "any-id");
        assert!(found.is_none());
    }

    // ---- extract_note_slots ----

    #[test]
    fn extract_note_slots_finds_single_slot() {
        let slots = extract_note_slots("Context: {{note1}}");
        assert_eq!(slots, vec!["note1"]);
    }

    #[test]
    fn extract_note_slots_finds_multiple_sorted() {
        let slots = extract_note_slots("{{note2}} then {{note1}}");
        assert_eq!(slots, vec!["note1", "note2"]);
    }

    #[test]
    fn extract_note_slots_deduplicates() {
        let slots = extract_note_slots("{{note1}} and {{note1}} again");
        assert_eq!(slots, vec!["note1"]);
    }

    #[test]
    fn extract_note_slots_ignores_transcript() {
        let slots = extract_note_slots("{{transcript}} only");
        assert!(slots.is_empty());
    }

    #[test]
    fn extract_note_slots_ignores_non_digit_suffix() {
        let slots = extract_note_slots("{{noteABC}}");
        assert!(slots.is_empty());
    }

    #[test]
    fn extract_note_slots_returns_empty_for_no_slots() {
        let slots = extract_note_slots("Just plain text.");
        assert!(slots.is_empty());
    }

    // ---- render_template ----

    #[test]
    fn render_template_replaces_transcript_only() {
        let notes = HashMap::new();
        let result = render_template("Say: {{transcript}}", "hello", &notes);
        assert_eq!(result.unwrap(), "Say: hello");
    }

    #[test]
    fn render_template_replaces_single_note_slot() {
        let mut notes = HashMap::new();
        notes.insert("note1".to_string(), "Meeting context.".to_string());
        let result = render_template("Context: {{note1}}", "", &notes);
        assert_eq!(result.unwrap(), "Context: Meeting context.");
    }

    #[test]
    fn render_template_replaces_multiple_slots() {
        let mut notes = HashMap::new();
        notes.insert("note1".to_string(), "First note.".to_string());
        notes.insert("note2".to_string(), "Second note.".to_string());
        let result = render_template("A: {{note1}}\nB: {{note2}}", "", &notes);
        let rendered = result.unwrap();
        assert!(rendered.contains("A: First note."));
        assert!(rendered.contains("B: Second note."));
    }

    #[test]
    fn render_template_replaces_transcript_and_notes() {
        let mut notes = HashMap::new();
        notes.insert("note1".to_string(), "Prior notes.".to_string());
        let result = render_template(
            "Transcript: {{transcript}}\nContext: {{note1}}",
            "Hello world",
            &notes,
        );
        let rendered = result.unwrap();
        assert!(rendered.contains("Transcript: Hello world"));
        assert!(rendered.contains("Context: Prior notes."));
    }

    #[test]
    fn render_template_errors_on_unresolved_slot() {
        let notes = HashMap::new();
        let result = render_template("See: {{note1}}", "", &notes);
        let err = result.unwrap_err();
        assert!(err.contains("note1"));
    }

    #[test]
    fn render_template_no_placeholders() {
        let notes = HashMap::new();
        let result = render_template("No placeholders.", "", &notes);
        assert_eq!(result.unwrap(), "No placeholders.");
    }

    #[test]
    fn render_template_same_note_in_multiple_slots() {
        let mut notes = HashMap::new();
        notes.insert("note1".to_string(), "Same content.".to_string());
        // note1 appears twice in the prompt
        let result = render_template("A: {{note1}} B: {{note1}}", "", &notes);
        assert_eq!(result.unwrap(), "A: Same content. B: Same content.");
    }
}
