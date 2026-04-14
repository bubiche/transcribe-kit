use std::{fs, path::PathBuf};

use directories::ProjectDirs;

use crate::{
    models::{Note, NoteSummary},
    settings::SettingsError,
};

/// Generate a unique note ID using UUIDv7 (time-ordered, random).
/// Format: "note-{uuidv7}"
pub fn generate_note_id() -> String {
    format!("note-{}", uuid::Uuid::now_v7())
}

/// Generate an ISO 8601 UTC timestamp string using only std.
/// Output format: "2026-04-11T14:30:00Z" (second precision, sufficient for sorting).
pub fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Manual conversion from epoch seconds to date-time components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to year/month/day (civil calendar)
    let (year, month, day) = epoch_days_to_date(days as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn epoch_days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant's date library (public domain)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[derive(Debug, Clone)]
pub struct NoteStore {
    notes_dir: PathBuf,
}

impl NoteStore {
    pub fn new() -> Result<Self, SettingsError> {
        let project_dirs = ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit")
            .ok_or(SettingsError::MissingConfigDir)?;
        Ok(Self {
            notes_dir: project_dirs.config_dir().join("notes"),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_dir(dir: PathBuf) -> Self {
        Self { notes_dir: dir }
    }

    pub fn list(&self) -> Vec<NoteSummary> {
        if !self.notes_dir.is_dir() {
            return Vec::new();
        }

        let entries = match fs::read_dir(&self.notes_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut summaries: Vec<NoteSummary> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    return None;
                }
                let contents = fs::read_to_string(&path).ok()?;
                match serde_json::from_str::<Note>(&contents) {
                    Ok(note) => Some(note.to_summary()),
                    Err(error) => {
                        eprintln!(
                            "Warning: skipping malformed note file {}: {error}",
                            path.display()
                        );
                        None
                    }
                }
            })
            .collect();

        // Sort by updated_at descending (newest first)
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    pub fn get(&self, id: &str) -> Option<Note> {
        // Reject path traversal
        if id.contains('/') || id.contains('\\') || id.contains("..") {
            return None;
        }

        let path = self.notes_dir.join(format!("{id}.json"));
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    pub fn save(&self, note: &Note) -> Result<Note, SettingsError> {
        if note.id.contains('/') || note.id.contains('\\') || note.id.contains("..") {
            return Err(SettingsError::Validation(
                "Invalid note ID: must not contain path separators or '..'".to_string(),
            ));
        }
        fs::create_dir_all(&self.notes_dir).map_err(SettingsError::CreateDirectory)?;

        let contents = serde_json::to_string_pretty(note).expect("note serialization is valid");
        let path = self.notes_dir.join(format!("{}.json", note.id));
        fs::write(path, contents).map_err(SettingsError::WriteFile)?;
        Ok(note.clone())
    }

    pub fn delete(&self, id: &str) -> Result<(), SettingsError> {
        // Reject path traversal
        if id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(SettingsError::Validation(
                "Invalid note ID: must not contain path separators or '..'".to_string(),
            ));
        }

        let path = self.notes_dir.join(format!("{id}.json"));
        if path.exists() {
            fs::remove_file(path).map_err(SettingsError::WriteFile)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::models::NoteSource;

    fn temp_store() -> (TempDir, NoteStore) {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = NoteStore::with_dir(temp_dir.path().join("notes"));
        (temp_dir, store)
    }

    fn sample_note(id: &str, title: &str, updated_at: &str) -> Note {
        Note {
            id: id.to_string(),
            title: title.to_string(),
            content: format!("Content of {title}"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            source: NoteSource::Manual,
        }
    }

    #[test]
    fn list_returns_empty_when_dir_missing() {
        let (_temp_dir, store) = temp_store();
        let result = store.list();
        assert!(result.is_empty());
    }

    #[test]
    fn list_returns_empty_when_dir_is_empty() {
        let (_temp_dir, store) = temp_store();
        fs::create_dir_all(&store.notes_dir).expect("create dir");
        let result = store.list();
        assert!(result.is_empty());
    }

    #[test]
    fn round_trip_save_then_get() {
        let (_temp_dir, store) = temp_store();
        let note = sample_note("note-1", "Test Note", "2026-04-11T10:00:00Z");
        store.save(&note).expect("save");

        let loaded = store.get("note-1").expect("get returns Some");
        assert_eq!(loaded.id, note.id);
        assert_eq!(loaded.title, note.title);
        assert_eq!(loaded.content, note.content);
        assert_eq!(loaded.created_at, note.created_at);
        assert_eq!(loaded.updated_at, note.updated_at);
    }

    #[test]
    fn save_creates_directory_if_missing() {
        let (_temp_dir, store) = temp_store();
        assert!(!store.notes_dir.exists());

        let note = sample_note("note-1", "Test", "2026-01-01T00:00:00Z");
        store.save(&note).expect("save");

        assert!(store.notes_dir.exists());
    }

    #[test]
    fn list_returns_summaries_sorted_by_updated_at() {
        let (_temp_dir, store) = temp_store();

        let old = sample_note("note-old", "Old", "2026-01-01T00:00:00Z");
        let mid = sample_note("note-mid", "Mid", "2026-06-15T12:00:00Z");
        let new = sample_note("note-new", "New", "2026-12-31T23:59:59Z");

        store.save(&old).expect("save old");
        store.save(&mid).expect("save mid");
        store.save(&new).expect("save new");

        let summaries = store.list();
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].id, "note-new");
        assert_eq!(summaries[1].id, "note-mid");
        assert_eq!(summaries[2].id, "note-old");
    }

    #[test]
    fn list_skips_malformed_files() {
        let (_temp_dir, store) = temp_store();

        let valid = sample_note("note-valid", "Valid", "2026-01-01T00:00:00Z");
        store.save(&valid).expect("save valid");

        // Write a corrupt JSON file
        let corrupt_path = store.notes_dir.join("note-corrupt.json");
        fs::write(corrupt_path, "not valid json {{{").expect("write corrupt");

        let summaries = store.list();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "note-valid");
    }

    #[test]
    fn delete_removes_file() {
        let (_temp_dir, store) = temp_store();

        let note = sample_note("note-del", "Delete Me", "2026-01-01T00:00:00Z");
        store.save(&note).expect("save");
        assert!(store.get("note-del").is_some());

        store.delete("note-del").expect("delete");
        assert!(store.get("note-del").is_none());
    }

    #[test]
    fn delete_is_idempotent() {
        let (_temp_dir, store) = temp_store();
        // Deleting a non-existent ID should succeed
        let result = store.delete("nonexistent-id");
        assert!(result.is_ok());
    }

    #[test]
    fn get_rejects_path_traversal() {
        let (_temp_dir, store) = temp_store();
        assert!(store.get("../etc/passwd").is_none());
        assert!(store.get("foo/bar").is_none());
        assert!(store.get("foo\\bar").is_none());
    }

    #[test]
    fn delete_rejects_path_traversal() {
        let (_temp_dir, store) = temp_store();
        assert!(store.delete("../etc/passwd").is_err());
        assert!(store.delete("foo/bar").is_err());
        assert!(store.delete("foo\\bar").is_err());
    }

    #[test]
    fn generate_note_id_is_unique() {
        let id1 = generate_note_id();
        let id2 = generate_note_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn iso_now_produces_valid_format() {
        let ts = iso_now();
        // Match YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
        assert_eq!(&ts[19..20], "Z");
    }

    #[test]
    fn to_summary_drops_content() {
        let note = sample_note("note-1", "Title", "2026-01-01T00:00:00Z");
        let summary = note.to_summary();

        assert_eq!(summary.id, note.id);
        assert_eq!(summary.title, note.title);
        assert_eq!(summary.created_at, note.created_at);
        assert_eq!(summary.updated_at, note.updated_at);
        // NoteSummary has no content field — compilation proves it's dropped
    }
}
