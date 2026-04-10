use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use directories::ProjectDirs;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::providers::local_llm;

/// Manages cancellation of an in-flight post-processing request.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) because the lock is
/// never held across an await — we only swap the token in and out.
#[derive(Clone)]
pub struct PostprocessCancelState {
    pub token: Arc<std::sync::Mutex<Option<CancellationToken>>>,
}

impl PostprocessCancelState {
    pub fn new() -> Self {
        Self {
            token: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

/// Tracks a running llama-server sidecar process.
#[allow(dead_code)] // `pid` is written for orphan cleanup via PID file
struct RunningServer {
    child: CommandChild,
    port: u16,
    model_id: String,
    pid: u32,
}

/// Manages the llama-server sidecar lifecycle.
///
/// Uses `tokio::sync::Mutex` (not `std::sync::Mutex`) because the lock
/// is held across async operations (spawning, health polling).
#[derive(Clone)]
pub struct LlmServerState {
    inner: Arc<Mutex<Option<RunningServer>>>,
}

impl LlmServerState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

/// Ensure a llama-server is running for the given model.
/// If already running with the correct model, return the port.
/// If running with a different model, kill it and start a new one.
/// If not running, start one.
pub async fn ensure_server_running(
    state: &LlmServerState,
    app_handle: &tauri::AppHandle,
    model_id: &str,
) -> Result<u16, String> {
    let mut guard = state.inner.lock().await;

    // Already running with correct model? Verify it's still alive.
    if let Some(ref server) = *guard {
        if server.model_id == model_id && is_healthy(server.port).await {
            return Ok(server.port);
        }
    }

    // Kill existing if any (wrong model or dead)
    if let Some(server) = guard.take() {
        let _ = server.child.kill();
        clear_sidecar_pid();
    }

    // Pick an available port
    let port = pick_available_port()?;

    // Resolve model path (must already be downloaded)
    let model_path = local_llm::resolve_model_path(model_id).map_err(|e| e.to_string())?;

    // Spawn sidecar
    let (mut rx, child) = app_handle
        .shell()
        .sidecar("llama-server")
        .map_err(|e| format!("Failed to create llama-server command: {e}"))?
        .args([
            "-m",
            &model_path.to_string_lossy(),
            "--port",
            &port.to_string(),
            "--host",
            "127.0.0.1",
            "-ngl",
            "99",            // offload all layers to GPU (silently ignored on CPU-only builds)
            "--jinja",       // enable full Jinja chat template support
            "--log-disable", // suppress verbose logging
        ])
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {e}"))?;

    let pid = child.pid();

    // Record PID for orphan cleanup
    save_sidecar_pid(pid);

    // Spawn task to drain stdout/stderr (prevents pipe buffer deadlock)
    tokio::spawn(async move {
        use CommandEvent::*;
        while let Some(event) = rx.recv().await {
            match event {
                Stderr(line) => {
                    eprintln!("[llama-server] {}", String::from_utf8_lossy(&line));
                }
                Terminated(payload) => {
                    eprintln!("[llama-server] exited: code={:?}", payload.code);
                    break;
                }
                _ => {}
            }
        }
    });

    // Poll /health until ready (timeout after 60s for large models).
    // On failure, kill the sidecar we just spawned to avoid orphaning it.
    if let Err(e) = poll_health(port, Duration::from_secs(60)).await {
        let _ = child.kill();
        clear_sidecar_pid();
        return Err(e);
    }

    *guard = Some(RunningServer {
        child,
        port,
        model_id: model_id.to_string(),
        pid,
    });

    Ok(port)
}

/// Kill the running server only if it is serving the given model.
/// Used before deleting a model's GGUF file — on Windows the file is
/// memory-mapped and cannot be removed while the sidecar holds it open.
pub async fn stop_server_for_model(state: &LlmServerState, model_id: &str) {
    let mut guard = state.inner.lock().await;
    let should_stop = guard.as_ref().is_some_and(|s| s.model_id == model_id);
    if should_stop {
        if let Some(server) = guard.take() {
            let _ = server.child.kill();
            clear_sidecar_pid();
        }
    }
}

/// Preload the llama-server sidecar at app startup if the user has
/// post-processing set to LocalLlm and the model is already downloaded.
/// Runs in a background tokio task so it doesn't block the UI.
pub fn preload_llm_server(
    state: LlmServerState,
    settings_store: crate::settings::SettingsStore,
    app_handle: tauri::AppHandle,
) {
    tauri::async_runtime::spawn(async move {
        let Ok(settings) = settings_store.load() else {
            return;
        };
        if settings.postprocess_provider_mode != crate::models::PostprocessProviderMode::LocalLlm {
            return;
        }

        // Only preload if the GGUF model is already downloaded
        if local_llm::expected_model_path(&settings.local_llm_model_id)
            .map(|p| p.exists())
            .unwrap_or(false)
        {
            let _ = ensure_server_running(&state, &app_handle, &settings.local_llm_model_id).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Port selection
// ---------------------------------------------------------------------------

/// Bind to port 0 to get an OS-assigned ephemeral port, then release it.
fn pick_available_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find available port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?
        .port();
    Ok(port)
}

// ---------------------------------------------------------------------------
// Health polling
// ---------------------------------------------------------------------------

/// Single non-blocking health check — returns true if server responds 200.
async fn is_healthy(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    matches!(
        reqwest::get(&url).await,
        Ok(resp) if resp.status().is_success()
    )
}

async fn poll_health(port: u16, timeout: Duration) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() > deadline {
            return Err("llama-server failed to become ready within timeout".to_string());
        }

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            // HTTP 503 = still loading, any error = not ready yet
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Chat completion (streaming SSE)
// ---------------------------------------------------------------------------

/// Send a streaming chat completion request to the running llama-server.
/// Collects all streamed tokens into a final string.
///
/// Uses `"stream": true` so that llama-server checks `is_connection_closed`
/// between tokens — dropping the connection on cancellation stops generation
/// promptly. During initial prompt processing (before any tokens stream),
/// cancellation is not possible; for typical prompt sizes this is sub-second.
pub async fn send_chat_completion(
    port: u16,
    prompt: &str,
    cancel_token: CancellationToken,
    enable_thinking: bool,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");

    let body = serde_json::json!({
        "messages": [
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.3,
        "stream": true,
        // Thinking mode — controlled by the caller. Models that don't
        // support this parameter (e.g. Gemma 4) simply ignore it.
        "chat_template_kwargs": { "enable_thinking": enable_thinking }
    });

    // Start the request — cancellable during connection phase
    let mut response = {
        let req = client.post(&url).json(&body).send();
        tokio::select! {
            result = req => result.map_err(|e| format!("HTTP request failed: {e}"))?,
            _ = cancel_token.cancelled() => {
                return Err("Post-processing was cancelled.".to_string());
            }
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("llama-server returned HTTP {status}: {err_body}"));
    }

    // Read SSE stream, collecting content deltas.
    // Buffer handles SSE lines that may be split across HTTP chunks.
    let mut result = String::new();
    let mut buf = String::new();

    loop {
        let chunk = tokio::select! {
            c = response.chunk() => c.map_err(|e| format!("Stream error: {e}"))?,
            _ = cancel_token.cancelled() => {
                // Dropping response closes the TCP connection.
                // llama-server detects this and stops generation.
                return Err("Post-processing was cancelled.".to_string());
            }
        };

        let Some(bytes) = chunk else { break };
        buf.push_str(&String::from_utf8_lossy(&bytes));

        // Process all complete lines in the buffer
        while let Some(newline_pos) = buf.find('\n') {
            let line: String = buf.drain(..=newline_pos).collect();
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(result.trim().to_string());
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // Only collect "content" tokens — ignore "reasoning_content"
                    // from thinking models so the internal chain-of-thought
                    // doesn't leak into the user-visible result.
                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                        result.push_str(content);
                    }
                }
            }
        }
    }

    Ok(result.trim().to_string())
}

// ---------------------------------------------------------------------------
// Orphan process cleanup (PID file)
// ---------------------------------------------------------------------------

const PID_FILE: &str = "llama-server.pid";

fn pid_file_path() -> Option<PathBuf> {
    ProjectDirs::from("dev", "transcribe-kit", "transcribe-kit")
        .map(|dirs| dirs.cache_dir().join(PID_FILE))
}

fn save_sidecar_pid(pid: u32) {
    if let Some(path) = pid_file_path() {
        let _ = std::fs::write(&path, pid.to_string());
    }
}

fn clear_sidecar_pid() {
    if let Some(path) = pid_file_path() {
        let _ = std::fs::remove_file(&path);
    }
}

/// Call on app startup to kill any orphaned llama-server from a previous crash.
pub fn cleanup_orphaned_sidecar() {
    let Some(path) = pid_file_path() else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        return;
    };

    #[cfg(unix)]
    {
        use std::process::Command;
        // Check if process exists
        let exists = unsafe { libc::kill(pid as i32, 0) } == 0;
        if exists {
            // Verify it's llama-server by checking the process name
            if let Ok(output) = Command::new("ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output()
            {
                let name = String::from_utf8_lossy(&output.stdout);
                if name.trim().contains("llama-server") {
                    let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                }
            }
        }
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        // Verify it's llama-server before killing (avoid killing unrelated PID reuse)
        if let Ok(output) = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
        {
            let info = String::from_utf8_lossy(&output.stdout);
            if info.contains("llama-server") {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }
    }

    let _ = std::fs::remove_file(&path);
}
