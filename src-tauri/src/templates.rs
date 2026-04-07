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
}
