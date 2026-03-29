# Architecture Overview

## App Layers

- Leptos frontend for UX and state management in Rust
- Tauri + Rust runtime for OS integrations, global hotkeys, audio, and local inference orchestration
- Provider adapters for local models (Whisper, Parakeet) and API models (OpenAI-compatible)

## Key Runtime Paths

- Audio file transcription path
- Live mic capture path with push-to-talk or toggle
- Post-processing path with prompt templates and custom instructions

## Planned Modules

- `frontend/src/features/audio.rs`: device selection and recording controls
- `frontend/src/features/transcription.rs`: model/provider selection and transcript display
- `frontend/src/features/postprocess.rs`: prompt templates and output controls
- `frontend/src/app.rs`: primary desktop shell UI
- `src-tauri/src/providers`: local and API transcription adapters
- `src-tauri/src/commands.rs`: frontend-callable command surface

## Build Tooling

- `cargo` manages both crates in one workspace
- `trunk` serves and builds the Leptos client-side app
- `cargo tauri` wraps the desktop build lifecycle
