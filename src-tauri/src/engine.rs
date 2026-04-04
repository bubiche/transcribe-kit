use std::sync::{Arc, Mutex};

use crate::models::ProviderMode;
use crate::providers::{local_whisper, local_whisper::WhisperEngine};
use crate::settings::SettingsStore;

#[derive(Clone)]
pub struct LocalEngineState {
    pub inner: Arc<Mutex<Option<WhisperEngine>>>,
}

impl LocalEngineState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

pub(crate) fn get_or_load_engine(
    engine_cache: &Arc<Mutex<Option<WhisperEngine>>>,
    model_id: &str,
) -> Result<WhisperEngine, String> {
    let mut guard = engine_cache.lock().unwrap();
    if let Some(ref engine) = *guard {
        if engine.model_id() == model_id {
            return Ok(engine.clone());
        }
    }

    let model_path = local_whisper::resolve_model_path(model_id).map_err(|e| e.to_string())?;
    let path_str = model_path
        .to_str()
        .ok_or("Model path contains invalid UTF-8")?;

    let engine = WhisperEngine::load(path_str, model_id.to_string()).map_err(|e| e.to_string())?;

    *guard = Some(engine.clone());

    Ok(engine)
}

pub fn preload_saved_local_model(engine_state: LocalEngineState, settings_store: SettingsStore) {
    std::thread::spawn(move || {
        let Ok(settings) = settings_store.load() else {
            return;
        };

        if settings.provider_mode != ProviderMode::Local {
            return;
        }

        let _ = get_or_load_engine(&engine_state.inner, &settings.local_model_id);
    });
}
