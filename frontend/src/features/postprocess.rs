use leptos::prelude::*;

#[component]
pub fn PostProcessFeatureCard() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Post-process"</p>
            <h3>"Prompt-driven cleanup"</h3>
            <ul class="list">
                <li>"User-authored prompt templates"</li>
                <li>"Optional cleanup, formatting, and rewriting passes"</li>
                <li>"Transcript-in, transformed-text-out pipeline"</li>
            </ul>
        </section>
    }
}
