use leptos::prelude::*;

#[component]
pub fn TranscribeScreen() -> impl IntoView {
    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Transcription"</h2>
                <p>"Record from a microphone or import audio files for transcription."</p>
            </div>

            <section class="section placeholder-section">
                <p class="tag">"Coming soon"</p>
                <h3>"Provider abstraction"</h3>
                <ul class="list">
                    <li>"Local Whisper models"</li>
                    <li>"Local NVIDIA Parakeet TDT 0.6B v3"</li>
                    <li>"OpenAI and OpenAI-compatible transcription APIs"</li>
                </ul>
            </section>
        </section>
    }
}
