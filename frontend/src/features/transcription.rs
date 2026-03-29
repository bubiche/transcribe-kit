use leptos::prelude::*;

#[component]
pub fn TranscriptionFeatureCard() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Transcription"</p>
            <h3>"Provider abstraction"</h3>
            <ul class="list">
                <li>"Local Whisper models"</li>
                <li>"Local NVIDIA Parakeet TDT 0.6B v3"</li>
                <li>"OpenAI and OpenAI-compatible transcription APIs"</li>
            </ul>
        </section>
    }
}

