use leptos::prelude::*;

#[component]
pub fn PostProcessScreen() -> impl IntoView {
    view! {
        <section class="panel content">
            <div class="hero">
                <h2>"Post-processing"</h2>
                <p>"Apply prompt-driven cleanup, formatting, and rewriting to transcripts."</p>
            </div>

            <section class="section placeholder-section">
                <p class="tag">"Coming soon"</p>
                <h3>"Prompt-driven cleanup"</h3>
                <ul class="list">
                    <li>"User-authored prompt templates"</li>
                    <li>"Optional cleanup, formatting, and rewriting passes"</li>
                    <li>"Transcript-in, transformed-text-out pipeline"</li>
                </ul>
            </section>
        </section>
    }
}
