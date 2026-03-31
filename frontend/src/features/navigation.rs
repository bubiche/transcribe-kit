use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Transcribe,
    PostProcess,
    Settings,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::Transcribe => "Transcribe",
            Screen::PostProcess => "Post-process",
            Screen::Settings => "Settings",
        }
    }

    const ALL: [Screen; 3] = [Screen::Transcribe, Screen::PostProcess, Screen::Settings];
}

#[component]
pub fn AppSidebar(active: RwSignal<Screen>) -> impl IntoView {
    view! {
        <aside class="panel sidebar">
            <h1 class="brand">"Transcribe Kit"</h1>
            <p class="lede">"Local and API-powered transcription with prompt-driven post-processing."</p>

            <nav class="nav">
                {Screen::ALL
                    .into_iter()
                    .map(|screen| {
                        let is_active = move || active.get() == screen;
                        view! {
                            <button
                                class="nav-chip"
                                class:nav-chip-active=is_active
                                aria-current=move || if is_active() { Some("page") } else { None }
                                on:click=move |_| active.set(screen)
                            >
                                {screen.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </nav>
        </aside>
    }
}
