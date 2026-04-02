use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

use crate::features::{
    navigation::{AppSidebar, Screen},
    postprocess::PostProcessScreen,
    settings::SettingsScreen,
    transcription::TranscribeScreen,
};
use crate::tauri_api::{listen_to_app_event, HotkeyActivityEvent, HotkeyActivityState, HotkeyMode};

const HOTKEY_ACTIVITY_EVENT_NAME: &str = "transcribe-kit://live-recording-hotkey";

#[component]
pub fn App() -> impl IntoView {
    let active_screen = RwSignal::new(Screen::Settings);
    let hotkey_activity = RwSignal::new(None::<HotkeyActivityEvent>);
    let activity_nonce = RwSignal::new(0_u64);

    Effect::new(move |_| {
        spawn_local(async move {
            let activity_signal = hotkey_activity;
            let nonce_signal = activity_nonce;
            let _ = listen_to_app_event(HOTKEY_ACTIVITY_EVENT_NAME, move |value: JsValue| {
                let Ok(event) = serde_wasm_bindgen::from_value::<HotkeyActivityEvent>(value) else {
                    return;
                };

                activity_signal.set(Some(event));
                let next_nonce = nonce_signal.get_untracked().saturating_add(1);
                nonce_signal.set(next_nonce);

                let clear_signal = activity_signal;
                let clear_nonce_signal = nonce_signal;
                let timeout_closure = Closure::once_into_js(move || {
                    if clear_nonce_signal.get_untracked() == next_nonce {
                        clear_signal.set(None);
                    }
                });

                if let Some(window) = web_sys::window() {
                    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        timeout_closure.as_ref().unchecked_ref(),
                        1800,
                    );
                }
            })
            .await;
        });
    });

    view! {
        <main class="shell">
            <HotkeyActivityBanner activity=hotkey_activity />
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

#[component]
fn HotkeyActivityBanner(activity: RwSignal<Option<HotkeyActivityEvent>>) -> impl IntoView {
    let label = Signal::derive(move || {
        let Some(activity) = activity.get() else {
            return String::new();
        };

        let mode_label = match activity.mode {
            HotkeyMode::PushToTalk => "push-to-talk",
            HotkeyMode::Toggle => "toggle",
        };
        let state_label = match activity.state {
            HotkeyActivityState::Pressed => "pressed",
            HotkeyActivityState::Released => "released",
        };

        if activity.triggered_while_background {
            format!(
                "Hotkey {state_label} in background: {} ({mode_label})",
                activity.shortcut
            )
        } else {
            format!("Hotkey {state_label}: {} ({mode_label})", activity.shortcut)
        }
    });

    view! {
        <Show when=move || activity.get().is_some()>
            <div
                class="hotkey-banner"
                class:hotkey-banner-background=move || activity.get().map(|event| event.triggered_while_background).unwrap_or(false)
            >
                <div class="hotkey-banner-dot"></div>
                <p class="hotkey-banner-copy">{move || label.get()}</p>
            </div>
        </Show>
    }
}
