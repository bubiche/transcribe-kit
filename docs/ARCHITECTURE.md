# Architecture Overview

## App Layers

- **Leptos frontend** (WASM) for UX, reactive state management, and user interaction
- **Tauri + Rust backend** for OS integrations, global hotkeys, audio capture, and local inference orchestration
- **Provider adapters** for local transcription (Whisper.cpp via whisper-rs), local LLM post-processing (llama-server sidecar), and cloud APIs (OpenAI-compatible endpoints)

## Workspace Layout

```
transcribe-kit/
├── Cargo.toml               # Workspace root (members: frontend, src-tauri)
├── Makefile.toml             # cargo-make task runner (dev, build, test, clippy)
├── Trunk.toml                # WASM bundler config (serves on :1420)
├── frontend/                 # Leptos CSR frontend crate
│   ├── index.html
│   ├── styles/
│   └── src/
└── src-tauri/                # Tauri backend crate
    ├── tauri.conf.json
    └── src/
```

## Frontend (`frontend/src/`)

Leptos client-side rendered app compiled to WebAssembly. Communicates with the backend via `wasm-bindgen` FFI calls to Tauri's invoke system.

```
src/
├── main.rs                          # Entry point, mounts App to DOM
├── app.rs                           # Root component, screen routing, hotkey/recording banners
├── tauri_api.rs                     # Backend bridge: all IPC types, commands, and event listeners
├── features/
│   ├── mod.rs                       # Re-exports feature modules
│   ├── transcription.rs             # Transcription screen (file picker, provider check, job launch)
│   ├── transcription/
│   │   ├── controller.rs            # TranscriptionController: job state signals, stream event handling
│   │   ├── panels.rs                # JobStatusPanel, TranscriptResultPanel (copy, timestamps, "Process with AI" navigation)
│   │   └── utils.rs                 # Formatting helpers (timestamps, durations, filenames)
│   ├── postprocess.rs               # Standalone Process screen (ProcessScreen): editable text input, template CRUD, note slot assignments, LLM execution, "Save as note"
│   ├── notes/
│   │   ├── mod.rs
│   │   ├── screen.rs                # NotesScreen: master-detail layout, source filter
│   │   ├── list.rs                  # NoteListPanel: list rendering, source filter chips, date formatting
│   │   └── editor.rs                # NoteEditorPanel: title/content editing, save/delete
│   └── settings/
│       ├── mod.rs
│       ├── screen.rs                # Settings panel: layout, effects (auto-save, preload)
│       ├── state.rs                 # SettingsFeatureState: form signals, debounced save, download tracking
│       ├── components.rs            # Provider/model/device/hotkey/API settings cards
│       ├── input_device_hints.rs    # Classify devices (physical mic, loopback, virtual cable, etc.)
│       └── meeting_capture.rs       # Platform-specific meeting readiness hints and guidance
└── live_recording/
    ├── mod.rs                       # LiveRecordingController: event listeners, goal reconciliation
    ├── device_context.rs            # Resolve armed device label, detect dual-capture readiness
    ├── recording_goal.rs            # Map hotkey events to Start/Stop commands (push-to-talk vs toggle)
    ├── timing.rs                    # Elapsed duration, wall-clock helpers
    ├── transcription_flow.rs        # State transitions on recording stop and transcription completion
    └── tests.rs
```

### State Management

- All mutable state uses Leptos `RwSignal<T>` (reactive read-write signals)
- Controller pattern: `TranscriptionController`, `LiveRecordingController`, `SettingsFeatureState` are `Copy + Clone` structs wrapping signals
- Data flows: user action -> signal update -> effect -> Tauri command -> response -> signal update -> re-render
- Backend events (`hotkey activity`, `recording status`, `audio levels`) are received via `listen_to_app_event()` and routed into signals

## Backend (`src-tauri/src/`)

Tauri v2 app with Rust backend. Manages audio I/O, model lifecycle, transcription providers, and persistent configuration.

```
src/
├── main.rs                          # Binary entry point
├── lib.rs                           # App init: state setup, plugin registration, command registration
├── models.rs                        # Shared data types (AppSettings, TranscriptResult, descriptors, etc.)
├── commands.rs                      # IPC command handlers exposed to the frontend
├── engine.rs                        # LocalEngineState: Whisper model cache (load once, reuse)
├── transcription.rs                 # Orchestration: route to local or API, decode audio, stream results
├── audio.rs                         # Multi-codec decoding (Symphonia + Opus fallback), MP3 encoding
├── llm_engine.rs                    # llama-server sidecar lifecycle, chat completion, cancellation
├── settings.rs                      # JSON config persistence + system keyring for API keys
├── templates.rs                     # Post-processing template storage and rendering
├── notes.rs                         # Notes storage: one JSON file per note, list/get/save/delete
├── hotkeys.rs                       # Global shortcut registration, press/release event emission
├── input_devices.rs                 # CPAL device enumeration, output loopback detection
├── audio_monitor.rs                 # Real-time RMS/peak audio level monitoring thread
├── recording_tray.rs                # System tray icon: idle/recording states, context menu
├── providers/
│   ├── mod.rs                       # Provider interface, TranscriptionError type
│   ├── local_whisper.rs             # Whisper.cpp adapter: model download, load, streaming inference
│   ├── api_openai_compatible.rs     # OpenAI-compatible API: transcription + chat completions
│   └── local_llm.rs                 # Local LLM model registry, GGUF download/cache, model metadata
└── live_recording/
    ├── mod.rs                       # LiveRecordingManager: single-stream and dual-stream capture
    └── wav_mixing.rs                # Mix mic + loopback WAV files, WAV metadata helpers
```

### Tauri-Managed State

All state is thread-safe (`Arc<Mutex<T>>`), injected via Tauri's state system:

| State | Purpose |
|-------|---------|
| `SettingsStore` | Persistent config + keyring access |
| `TemplateStore` | Post-processing templates |
| `NoteStore` | Persistent notes (one JSON file per note) |
| `LocalEngineState` | Cached Whisper model instance |
| `HotkeyManagerState` | Current hotkey binding and errors |
| `LiveRecordingManagerState` | Active recording session |
| `LlmServerState` | llama-server sidecar process (child handle, port, model ID, PID) |
| `PostprocessCancelState` | Cancellation token for in-flight post-processing |
| `AudioMonitorState` | Active audio monitor stream |

### Events Emitted to Frontend

| Event | Payload | Purpose |
|-------|---------|---------|
| `transcribe-kit://live-recording-status` | `LiveRecordingStatus` | Recording start/stop notifications |
| `transcribe-kit://live-recording-hotkey` | Shortcut, mode, state | Global hotkey press/release |
| `transcribe-kit://audio-level` | `{ rms, peak }` | Audio level meter updates (~66ms) |

## Key Runtime Paths

### File Transcription
1. User picks audio file via system dialog
2. Provider readiness check (model downloaded or API key present)
3. **Local path**: decode to 16kHz mono PCM -> Whisper inference with streaming segments
4. **API path**: compress to MP3 if >24MB -> upload to OpenAI-compatible endpoint
5. Display transcript with timestamps, copy, and post-process options

### Live Recording + Transcription
1. Global hotkey or UI button triggers recording goal
2. Goal reconciliation: desired state vs current state -> Start/Stop command
3. **Single stream**: capture from selected input device to WAV
4. **Dual stream** (meeting mix): capture mic + system loopback -> mix into single WAV
5. On stop: hand WAV to transcription pipeline (same local/API routing as file path)
6. Auto-navigate to transcript on completion
7. On success, the transcript is also auto-saved as a `Transcription`-source note

### Post-Processing (standalone Process tab)
1. User opens the Process tab directly (no prior transcription required). Input can be:
   - Typed/pasted into the editable input textarea
   - Loaded from a saved note via "Load from note"
   - Pre-filled by clicking "Process with AI" on a transcript result (sets the `pending_process_text` signal and switches `active_screen` to `Process`)
2. User selects or creates a template. Prompt placeholders:
   - `{{transcript}}` — replaced with the input textarea content
   - `{{noteN}}` — replaced with the content of the note assigned to that slot
3. Template rendered with input text + assigned note content
4. Route based on `postprocess_provider_mode`:
   - **API**: send to remote OpenAI-compatible chat completions endpoint
   - **Local LLM**: ensure llama-server sidecar is running with the selected model, then send to `http://127.0.0.1:<port>/v1/chat/completions`
5. Streaming response with cancellation support
6. Result displayed with copy / export / **"Save as note"** (manual — post-processing results no longer auto-save)

### Notes
1. Notes are CRUD-managed via Tauri commands: `list_notes`, `get_note`, `create_note`, `update_note`, `delete_note`
2. Storage: one JSON file per note at `~/.config/transcribe-kit/notes/<id>.json`. `list_notes` returns lightweight `NoteSummary` (no content) for fast list rendering; `get_note(id)` returns the full `Note`
3. `NoteSource` distinguishes how a note was created:
   - `Manual` — created from the Notes tab
   - `Transcription` — auto-saved when a transcription completes
   - `PostProcessing` — manually saved from the Process tab result
4. Notes are read from the Process tab in two ways: as full-content input (via "Load from note") and as named template slots (via `{{noteN}}` placeholders)

## Audio Pipeline

- **Decoding**: Symphonia (MP3, FLAC, WAV, M4A, WebM) + custom Opus/OGG fallback
- **Normalization**: all audio resampled to 16kHz mono f32 for Whisper
- **Compression**: re-encode to 64kbps MP3 for API uploads exceeding 24MB
- **Live capture**: CPAL streams -> mpsc channels -> WAV writer threads
- **Dual capture**: separate mic and loopback WAV files mixed post-recording

## Supported Whisper Models

| Model ID | Approx Size |
|----------|-------------|
| whisper-tiny | ~75 MB |
| whisper-base | ~148 MB |
| whisper-small | ~488 MB |
| whisper-large-v3-turbo | ~809 MB |

Downloaded from Hugging Face, cached at `~/.cache/transcribe-kit/models/`.

## Local LLM (llama-server Sidecar)

Post-processing can run on-device using a llama-server sidecar process from llama.cpp. Unlike the Whisper engine (linked in-process via whisper-rs), the LLM runs as a separate process to avoid ggml symbol conflicts between whisper-rs and llama-cpp.

```
Tauri App Process                          Sidecar Process
+------------------------------+           +-------------------------+
| Rust backend                 |           | llama-server            |
|   providers/local_llm.rs     |  HTTP     |   -m model.gguf         |
|     - model registry         | -------> |   --port <port>         |
|     - GGUF download/cache    | <------- |   --host 127.0.0.1      |
|                              |  JSON     |                         |
|   llm_engine.rs              |           | OpenAI-compatible API:  |
|     - LlmServerState         |           |   GET  /health          |
|     - start/stop sidecar     |           |   POST /v1/chat/compl.  |
|     - send_chat_completion() |           +-------------------------+
+------------------------------+
```

- **Lifecycle**: started on-demand when the user first runs local post-processing (or on preload). Stays running for subsequent requests. Killed on app exit or model switch.
- **Port**: OS-assigned ephemeral port via `TcpListener::bind("127.0.0.1:0")`. Localhost only.
- **Readiness**: polls `GET /health` until HTTP 200 with `{"status":"ok"}`.
- **Streaming**: uses `"stream": true` for SSE responses with cancellation support.
- **Orphan cleanup**: PID recorded on startup; orphaned processes from prior crashes are killed on next launch.

### Supported LLM Models

| Model ID | Model | GGUF Q4_K_M Size | Context | Notes |
|----------|-------|------------------|---------|-------|
| `llm-qwen-3.5-0.8b` (default) | Qwen 3.5 0.8B | ~0.50 GB | 32K | Smallest/fastest, excellent multilingual |
| `llm-qwen-3.5-4b` | Qwen 3.5 4B | ~2.55 GB | 32K | Better quality, still reasonable |
| `llm-gemma-4-e2b` | Gemma 4 E2B | ~2.89 GB | 128K | General purpose, long context |
| `llm-gemma-4-e4b` | Gemma 4 E4B | ~4.64 GB | 128K | Higher quality text processing |

Downloaded from Hugging Face, cached at `~/.cache/transcribe-kit/models/`.

### Sidecar Binaries

```
src-tauri/binaries/
  llama-server-aarch64-apple-darwin       (macOS ARM, Metal GPU)
  llama-server-x86_64-apple-darwin        (macOS Intel)
  llama-server-x86_64-unknown-linux-gnu   (Linux x64)
  llama-server-x86_64-pc-windows-msvc.exe (Windows x64)
```

Not committed to git — downloaded via `scripts/download-llama-server.sh` during local setup and CI.

## Build Tooling

- **cargo-make** (`Makefile.toml`): task runner for `dev`, `build`, `test`, `clippy`, `setup`
- **Trunk**: serves and builds the Leptos WASM frontend (port 1420)
- **cargo tauri**: wraps the full desktop build lifecycle
- **No JS tooling**: pure Rust ecosystem, no package.json/webpack/vite

### Commands

```sh
cargo make setup    # Install wasm32 target, Trunk, Tauri CLI
cargo make dev      # Run in dev mode (hot reload)
cargo make build    # Production build
cargo make test     # fmt-check + clippy + unit tests
```

## Persistence

- **Settings**: `~/.config/transcribe-kit/settings.json`
- **API keys**: system keyring (`dev.transcribekit.desktop` service)
- **Templates**: `~/.config/transcribe-kit/templates.json`
- **Notes**: `~/.config/transcribe-kit/notes/<id>.json` (one file per note)
- **Models**: `~/.cache/transcribe-kit/models/`
