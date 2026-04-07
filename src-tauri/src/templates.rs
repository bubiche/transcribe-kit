use std::{fs, path::PathBuf};

use directories::ProjectDirs;

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

/// Validate that a template prompt contains the `{{transcript}}` placeholder.
pub fn validate_template_placeholder(prompt: &str) -> Result<(), String> {
    if !prompt.contains("{{transcript}}") {
        return Err(
            "The selected template is missing the {{transcript}} placeholder in its prompt."
                .to_string(),
        );
    }
    Ok(())
}

/// Replace the `{{transcript}}` placeholder with the actual transcript text.
pub fn render_template_prompt(prompt: &str, transcript_text: &str) -> String {
    prompt.replace("{{transcript}}", transcript_text)
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

    // ---- validate_template_placeholder ----

    #[test]
    fn validate_placeholder_accepts_prompt_with_placeholder() {
        assert!(validate_template_placeholder("Summarize: {{transcript}}").is_ok());
    }

    #[test]
    fn validate_placeholder_accepts_placeholder_in_middle() {
        assert!(validate_template_placeholder("Before {{transcript}} after").is_ok());
    }

    #[test]
    fn validate_placeholder_rejects_prompt_without_placeholder() {
        let error = validate_template_placeholder("No placeholder here").unwrap_err();
        assert!(error.contains("{{transcript}}"));
    }

    #[test]
    fn validate_placeholder_rejects_empty_prompt() {
        assert!(validate_template_placeholder("").is_err());
    }

    #[test]
    fn validate_placeholder_rejects_partial_placeholder() {
        assert!(validate_template_placeholder("{{transcri}}").is_err());
        assert!(validate_template_placeholder("{transcript}").is_err());
    }

    // ---- render_template_prompt ----

    #[test]
    fn render_template_prompt_replaces_placeholder_with_transcript() {
        let rendered = render_template_prompt("Summarize: {{transcript}}", "Hello world");
        assert_eq!(rendered, "Summarize: Hello world");
    }

    #[test]
    fn render_template_prompt_handles_empty_transcript() {
        let rendered = render_template_prompt("Summarize: {{transcript}}", "");
        assert_eq!(rendered, "Summarize: ");
    }

    #[test]
    fn render_template_prompt_replaces_multiple_occurrences() {
        let rendered =
            render_template_prompt("First: {{transcript}}\nSecond: {{transcript}}", "text");
        assert_eq!(rendered, "First: text\nSecond: text");
    }

    #[test]
    fn render_template_prompt_preserves_special_characters_in_transcript() {
        let transcript = "He said \"hello\" & 'goodbye' <tag> $100";
        let rendered = render_template_prompt("{{transcript}}", transcript);
        assert_eq!(rendered, transcript);
    }

    #[test]
    fn render_template_prompt_handles_multiline_transcript() {
        let transcript = "Line 1\nLine 2\nLine 3";
        let rendered = render_template_prompt("Notes:\n{{transcript}}", transcript);
        assert_eq!(rendered, "Notes:\nLine 1\nLine 2\nLine 3");
    }
}
