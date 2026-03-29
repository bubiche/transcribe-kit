use leptos::prelude::*;

use crate::features::{
    audio::AudioFeatureCard, postprocess::PostProcessFeatureCard,
    transcription::TranscriptionFeatureCard,
};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="shell">
            <div class="frame">
                <aside class="panel sidebar">
                    <p class="tag">"Rust-first desktop app"</p>
                    <h1 class="brand">"Transcribe Kit"</h1>
                    <p class="lede">
                        "Cross-platform transcription with local models, API providers, and prompt-driven cleanup."
                    </p>

                    <div class="nav">
                        <div class="nav-chip">"Transcription engines"</div>
                        <div class="nav-chip">"Live audio controls"</div>
                        <div class="nav-chip">"Prompt post-processing"</div>
                        <div class="nav-chip">"Desktop settings"</div>
                    </div>
                </aside>

                <section class="panel content">
                    <div class="hero">
                        <h2>"Leptos + Tauri workspace ready"</h2>
                        <p>
                            "The frontend now lives in Rust. Next we can wire this shell into Tauri commands for model discovery, audio devices, hotkeys, and transcript execution."
                        </p>
                    </div>

                    <div class="status">
                        <div class="status-card">
                            <p class="status-label">"Frontend"</p>
                            <p class="status-value">"Leptos CSR via Trunk"</p>
                        </div>
                        <div class="status-card">
                            <p class="status-label">"Desktop runtime"</p>
                            <p class="status-value">"Tauri 2 + Rust"</p>
                        </div>
                        <div class="status-card">
                            <p class="status-label">"Providers"</p>
                            <p class="status-value">"Whisper, Parakeet, OpenAI-compatible"</p>
                        </div>
                    </div>

                    <div class="grid">
                        <TranscriptionFeatureCard />
                        <AudioFeatureCard />
                        <PostProcessFeatureCard />
                        <ProjectNotesCard />
                    </div>
                </section>
            </div>
        </main>
    }
}

#[component]
fn ProjectNotesCard() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Build notes"</p>
            <h3>"Implementation priorities"</h3>
            <ul class="list">
                <li>"Persist provider, model, device, and hotkey settings."</li>
                <li>"Implement file and live audio input paths in the Tauri layer."</li>
                <li>"Run transcript text through optional AI post-processing prompts."</li>
            </ul>
        </section>
    }
}

