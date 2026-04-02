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
use crate::live_recording::{
    format_duration, live_elapsed_duration_ms, LiveRecordingController, HOTKEY_ACTIVITY_EVENT_NAME,
};
use crate::tauri_api::{
    listen_to_app_event, HotkeyActivityEvent, HotkeyActivityState, HotkeyMode, LiveRecordingState,
};

#[component]
pub fn App() -> impl IntoView {
    let active_screen = RwSignal::new(Screen::Settings);
    let hotkey_activity = RwSignal::new(None::<HotkeyActivityEvent>);
    let activity_nonce = RwSignal::new(0_u64);
    let recording_elapsed_tick = RwSignal::new(0_u64);
    let live_recording = LiveRecordingController::new();

    Effect::new(move |_| {
        live_recording.initialize();
    });

    Effect::new(move |_| {
        let tick_signal = recording_elapsed_tick;
        let interval_closure = Closure::wrap(Box::new(move || {
            tick_signal.update(|tick| *tick = tick.saturating_add(1));
        }) as Box<dyn FnMut()>);

        if let Some(window) = web_sys::window() {
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                interval_closure.as_ref().unchecked_ref(),
                250,
            );
        }

        interval_closure.forget();
    });

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
            <LiveRecordingBanner controller=live_recording elapsed_tick=recording_elapsed_tick />
            <div class="frame">
                <AppSidebar active=active_screen />

                <div class="screen" class:screen-active=move || active_screen.get() == Screen::Transcribe>
                    <TranscribeScreen active=Signal::derive(move || active_screen.get() == Screen::Transcribe) />
                </div>
                <div class="screen" class:screen-active=move || active_screen.get() == Screen::PostProcess>
                    <PostProcessScreen />
                </div>
                <div class="screen" class:screen-active=move || active_screen.get() == Screen::Settings>
                    <SettingsScreen live_recording=live_recording />
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

#[component]
fn LiveRecordingBanner(
    controller: LiveRecordingController,
    elapsed_tick: RwSignal<u64>,
) -> impl IntoView {
    let live_elapsed_ms = Signal::derive(move || {
        elapsed_tick.get();
        live_elapsed_duration_ms(
            &controller.status.get(),
            controller.recording_started_at_ms.get(),
        )
    });

    let banner_class = Signal::derive(move || {
        if matches!(controller.status.get().state, LiveRecordingState::Recording) {
            "recording-banner recording-banner-active".to_string()
        } else if controller.error_message.get().is_some()
            || controller.load_error.get().is_some()
            || controller.device_context_error.get().is_some()
        {
            "recording-banner recording-banner-error".to_string()
        } else {
            "recording-banner".to_string()
        }
    });

    let state_label = Signal::derive(move || {
        if matches!(controller.status.get().state, LiveRecordingState::Recording) {
            "Recording".to_string()
        } else if controller.error_message.get().is_some()
            || controller.load_error.get().is_some()
            || controller.device_context_error.get().is_some()
        {
            "Needs attention".to_string()
        } else if controller.is_ready.get() {
            "Armed".to_string()
        } else {
            "Loading".to_string()
        }
    });

    let active_device_label = Signal::derive(move || {
        controller
            .status
            .get()
            .input_device_label
            .unwrap_or_else(|| controller.armed_input_label.get())
    });

    let detail_copy = Signal::derive(move || {
        if let Some(error) = controller.error_message.get() {
            return error;
        }

        if let Some(error) = controller.load_error.get() {
            return error;
        }

        if let Some(error) = controller.device_context_error.get() {
            return error;
        }

        if matches!(controller.status.get().state, LiveRecordingState::Recording) {
            let status = controller.status.get();
            return format!(
                "Recording from {} at {} Hz, {} ch, {} elapsed.",
                status
                    .input_device_label
                    .unwrap_or_else(|| controller.armed_input_label.get()),
                status.sample_rate_hz.unwrap_or_default(),
                status.channels.unwrap_or_default(),
                format_duration(live_elapsed_ms.get()),
            );
        }

        if let Some(message) = controller.feedback_message.get() {
            return message;
        }

        format!(
            "Ready to capture from {} when the recording hotkey is pressed.",
            controller.armed_input_label.get()
        )
    });

    view! {
        <section class=move || banner_class.get()>
            <div class="recording-banner-heading">
                <div class="recording-banner-status">
                    <div class="recording-banner-dot"></div>
                    <p class="recording-banner-title">{move || state_label.get()}</p>
                </div>
                <span class="mini-chip">{move || format!("Mic: {}", active_device_label.get())}</span>
            </div>

            <p class="recording-banner-copy">{move || detail_copy.get()}</p>

            <div class="mini-status">
                <Show when=move || matches!(controller.status.get().state, LiveRecordingState::Recording)>
                    <span class="mini-chip">
                        {move || {
                            controller
                                .status
                                .get()
                                .sample_rate_hz
                                .map(|sample_rate| format!("{sample_rate} Hz"))
                                .unwrap_or_else(|| "Sample rate pending".to_string())
                        }}
                    </span>
                    <span class="mini-chip">
                        {move || {
                            controller
                                .status
                                .get()
                                .channels
                                .map(|channels| format!("{channels} ch"))
                                .unwrap_or_else(|| "Channel count pending".to_string())
                        }}
                    </span>
                    <span class="mini-chip">
                        {move || {
                            format!(
                                "Elapsed: {}",
                                format_duration(live_elapsed_ms.get()),
                            )
                        }}
                    </span>
                </Show>

                <Show when=move || !matches!(controller.status.get().state, LiveRecordingState::Recording)>
                    <span class="mini-chip">
                        {move || format!("Selected: {}", controller.armed_input_label.get())}
                    </span>
                    <Show when=move || controller.last_result.get().is_some()>
                        <span class="mini-chip">
                            {move || {
                                controller
                                    .last_result
                                    .get()
                                    .map(|result| {
                                        format!(
                                            "Last capture: {}",
                                            format_duration(result.duration_ms),
                                        )
                                    })
                                    .unwrap_or_default()
                            }}
                        </span>
                    </Show>
                </Show>
            </div>
        </section>
    }
}
