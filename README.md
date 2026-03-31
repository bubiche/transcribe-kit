# Transcribe Kit

Cross-platform desktop transcription app scaffold built with Tauri + Leptos.

## Goal

Build an app similar to TypeWhisper but from a single shared codebase targeting:

- macOS
- Windows
- Linux

## Required Product Features

### 1) Multiple transcription backends

The app must support both:

- Local on-device transcription
- API-based transcription

Local model options:

- Whisper family
- NVIDIA Parakeet TDT 0.6B v3

API options:

- OpenAI transcription API
- OpenAI-compatible APIs via custom base URL

User requirements:

- User can choose local vs API mode
- User can choose model per mode
- User can set and update API key
- User can set and update API base URL

### 2) Audio input modes

The app must support both:

- Audio file input transcription
- Live microphone transcription

Live microphone requirements:

- Push-to-talk hotkey mode
- Toggle recording hotkey mode
- Input device selection

### 3) Post-processing pipeline

After base transcription is generated:

- Transcript can be sent to AI post-processing
- User can define custom prompts
- Prompt templates should be supported

### 4) One UI codebase for all desktop platforms

- Same UI implementation across macOS, Windows, and Linux
- Platform-specific behavior should live in the Tauri layer where needed

## Tech Stack

- `frontend/`: Leptos client-side UI written in Rust
- `Trunk`: Rust/WASM bundler and dev server
- `src-tauri/`: Rust + Tauri desktop runtime
- `src-tauri/src/providers/`: provider adapter stubs for Whisper, Parakeet, and OpenAI-compatible APIs

## Scaffold Included In This Repository

- Root Cargo workspace for both frontend and desktop runtime crates
- Leptos frontend shell with feature-oriented modules
- Tauri runtime with stub commands and provider adapters
- Collaboration docs that spell out product scope for future agents

## Collaboration Contract For Other Agents

Use this as project guardrails when implementing features.

1. Keep desktop-specific behavior in `src-tauri` unless it is purely presentation or local UI state.
2. Maintain provider abstraction so adding or removing engines does not force UI contract changes.
3. Do not hardcode API keys or secrets.
4. Preserve the ability to configure an OpenAI-compatible base URL.
5. Keep audio capture and transcription execution decoupled.
6. Every feature PR should update docs when contracts or architecture change.

## Suggested Build Order

1. Build settings state and persistence for provider, model, API key, base URL, hotkeys, and input device.
2. Implement audio capture and file import flows.
3. Implement the local Whisper path end-to-end.
4. Implement the OpenAI-compatible API path end-to-end.
5. Add the Parakeet local inference adapter.
6. Add the post-processing prompt pipeline.
7. Add packaging and release flow for macOS, Windows, and Linux.

## Local Development

Prerequisites:

- Rust stable
- `wasm32-unknown-unknown` target
- `trunk`
- `tauri-cli`
- `cargo-make`
- `cmake` (required to build whisper.cpp from source)
- A C/C++ compiler (Xcode Command Line Tools on macOS, MSVC on Windows, `gcc`/`g++` on Linux)
- Platform dependencies required by Tauri

Commands:

```bash
cargo install cargo-make --locked
cargo make setup
cargo make dev
```

Production build:

```bash
cargo make build
```

Useful task shortcuts:

- `cargo make setup`: install the WASM target plus required CLI tools
- `cargo make dev`: run the Tauri desktop app in development mode
- `cargo make build`: build production desktop bundles
- `cargo make build-frontend`: build the Leptos frontend only

## Why Leptos Here

This repo is intentionally Rust-first to reduce future JavaScript ecosystem maintenance. The UI is simple enough that Leptos and Trunk are a good fit, while native-heavy behavior like audio capture, hotkeys, settings persistence, and local model orchestration still live in Tauri.
