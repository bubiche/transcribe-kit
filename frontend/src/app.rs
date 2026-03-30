use crate::features::settings::SettingsScreen;
use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="shell">
            <SettingsScreen />
        </main>
    }
}
