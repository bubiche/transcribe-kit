use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, UserAttentionType};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{models::HotkeyMode, settings::SettingsStore};

pub const HOTKEY_EVENT_NAME: &str = "transcribe-kit://live-recording-hotkey";

#[derive(Debug, Clone)]
pub struct HotkeyManagerState {
    inner: Arc<Mutex<RegisteredHotkeyState>>,
}

#[derive(Debug, Clone, Default)]
struct RegisteredHotkeyState {
    shortcut: Option<String>,
    mode: HotkeyMode,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
enum HotkeyEventState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Serialize)]
struct HotkeyEventPayload {
    shortcut: String,
    mode: HotkeyMode,
    state: HotkeyEventState,
    triggered_while_background: bool,
}

impl HotkeyManagerState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RegisteredHotkeyState::default())),
        }
    }

    pub fn registration_error(&self) -> Option<String> {
        self.inner.lock().unwrap().last_error.clone()
    }

    pub fn initialize_from_settings<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        settings_store: &SettingsStore,
    ) {
        match settings_store.load() {
            Ok(settings) => {
                if let Err(error) = self.apply(app, &settings.hotkey_shortcut, settings.hotkey_mode)
                {
                    eprintln!("Failed to register saved hotkey: {error}");
                }
            }
            Err(error) => {
                self.set_error(Some(format!(
                    "Transcribe Kit could not load the saved hotkey settings: {error}"
                )));
            }
        }
    }

    pub fn apply<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        shortcut: &str,
        mode: HotkeyMode,
    ) -> Result<String, String> {
        let normalized_shortcut = validate_shortcut(shortcut)?;
        let previous_shortcut = self.inner.lock().unwrap().shortcut.clone();

        if previous_shortcut.as_deref() == Some(normalized_shortcut.as_str()) {
            let mut guard = self.inner.lock().unwrap();
            guard.mode = mode;
            guard.last_error = None;
            return Ok(normalized_shortcut);
        }

        let state = self.clone();
        app.global_shortcut()
            .on_shortcut(normalized_shortcut.as_str(), move |app, shortcut, event| {
                state.handle_event(app, shortcut.into_string(), event);
            })
            .map_err(|error| {
                let message = format_registration_error(&normalized_shortcut, error.to_string());
                self.set_error(Some(message.clone()));
                message
            })?;

        if let Some(previous_shortcut) = previous_shortcut.as_deref() {
            if let Err(error) = app.global_shortcut().unregister(previous_shortcut) {
                let _ = app
                    .global_shortcut()
                    .unregister(normalized_shortcut.as_str());
                let message = format!(
                    "Transcribe Kit could not replace the existing hotkey with \"{normalized_shortcut}\": {error}"
                );
                self.set_error(Some(message.clone()));
                return Err(message);
            }
        }

        let mut guard = self.inner.lock().unwrap();
        guard.shortcut = Some(normalized_shortcut.clone());
        guard.mode = mode;
        guard.last_error = None;

        Ok(normalized_shortcut)
    }

    fn handle_event<R: Runtime>(&self, app: &AppHandle<R>, shortcut: String, event: ShortcutEvent) {
        let state = match event.state {
            ShortcutState::Pressed => HotkeyEventState::Pressed,
            ShortcutState::Released => HotkeyEventState::Released,
        };

        let mut triggered_while_background = false;
        if let Some(main_window) = app.get_webview_window("main") {
            match main_window.is_focused() {
                Ok(true) => {
                    let _ = main_window.request_user_attention(None);
                }
                Ok(false) => {
                    triggered_while_background = true;
                    if matches!(state, HotkeyEventState::Pressed) {
                        let _ = main_window
                            .request_user_attention(Some(UserAttentionType::Informational));
                    }
                }
                Err(_) => {}
            }
        }

        let mode = self.inner.lock().unwrap().mode;
        let _ = app.emit(
            HOTKEY_EVENT_NAME,
            HotkeyEventPayload {
                shortcut,
                mode,
                state,
                triggered_while_background,
            },
        );
    }

    fn set_error(&self, error: Option<String>) {
        self.inner.lock().unwrap().last_error = error;
    }
}

pub fn validate_shortcut(shortcut: &str) -> Result<String, String> {
    let normalized = shortcut.trim();
    if normalized.is_empty() {
        return Err("Enter a hotkey before saving the recording shortcut.".to_string());
    }

    let parsed = Shortcut::from_str(normalized)
        .map_err(|error| format!("Enter a valid hotkey like CmdOrCtrl+Shift+T. {error}"))?;

    if parsed.mods.is_empty() {
        return Err(
            "Choose a hotkey with at least one modifier key so it does not trigger accidentally."
                .to_string(),
        );
    }

    Ok(normalized.to_string())
}

fn format_registration_error(shortcut: &str, error: String) -> String {
    format!(
        "The hotkey \"{shortcut}\" could not be registered globally. Another app may already be using it. Choose a different shortcut and save again. {error}"
    )
}

#[cfg(test)]
mod tests {
    use super::validate_shortcut;

    #[test]
    fn accepts_cmd_or_ctrl_shortcut() {
        let result = validate_shortcut("CmdOrCtrl+Shift+T");

        assert_eq!(result.as_deref(), Ok("CmdOrCtrl+Shift+T"));
    }

    #[test]
    fn rejects_shortcuts_without_modifiers() {
        let result = validate_shortcut("T");

        assert!(result
            .expect_err("shortcut should be rejected")
            .contains("modifier"));
    }

    #[test]
    fn rejects_blank_shortcuts() {
        assert!(validate_shortcut("   ").is_err());
    }
}
