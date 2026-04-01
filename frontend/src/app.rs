use leptos::prelude::*;

use crate::features::{
    navigation::{AppSidebar, Screen},
    postprocess::PostProcessScreen,
    settings::SettingsScreen,
    transcription::TranscribeScreen,
};

#[component]
pub fn App() -> impl IntoView {
    let active_screen = RwSignal::new(Screen::Settings);

    view! {
        <main class="shell">
            <div class="frame">
                <AppSidebar active=active_screen />

                <div class="screen" class:screen-active=move || active_screen.get() == Screen::Transcribe>
                    <TranscribeScreen active=Signal::derive(move || active_screen.get() == Screen::Transcribe) />
                </div>
                <div class="screen" class:screen-active=move || active_screen.get() == Screen::PostProcess>
                    <PostProcessScreen />
                </div>
                <div class="screen" class:screen-active=move || active_screen.get() == Screen::Settings>
                    <SettingsScreen />
                </div>
            </div>
        </main>
    }
}
