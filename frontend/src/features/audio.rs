use leptos::prelude::*;

#[component]
pub fn AudioFeatureCard() -> impl IntoView {
    view! {
        <section class="section">
            <p class="tag">"Audio"</p>
            <h3>"Live capture controls"</h3>
            <ul class="list">
                <li>"Push-to-talk and toggle recording modes"</li>
                <li>"Selectable input devices and permission prompts"</li>
                <li>"Shared path for mic streaming and file import"</li>
            </ul>
        </section>
    }
}
