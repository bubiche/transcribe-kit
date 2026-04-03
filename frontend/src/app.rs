use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

use crate::features::{
    navigation::{AppSidebar, Screen},
    postprocess::PostProcessScreen,
    settings::SettingsScreen,
    transcription::{TranscribeScreen, TranscriptionController},
};
use crate::live_recording::{
    format_duration, live_elapsed_duration_ms, LiveRecordingController, HOTKEY_ACTIVITY_EVENT_NAME,
};
use crate::tauri_api::{
    listen_to_app_event, HotkeyActivityEvent, HotkeyActivityState, HotkeyMode, InputType,
    LiveRecordingState,
};

#[component]
pub fn App() -> impl IntoView {
    let active_screen = RwSignal::new(Screen::Settings);
    let hotkey_activity = RwSignal::new(None::<HotkeyActivityEvent>);
    let activity_nonce = RwSignal::new(0_u64);
    let recording_elapsed_tick = RwSignal::new(0_u64);
    let transcription = TranscriptionController::new();
    let live_recording = LiveRecordingController::new(transcription);
    let live_recording_state = Signal::derive(move || live_recording.status.get().state);
    let live_recording_label = Signal::derive(move || {
        live_recording
            .status
            .get()
            .input_device_label
            .unwrap_or_else(|| live_recording.armed_input_label.get())
    });
    let live_recording_elapsed_ms = Signal::derive(move || {
        recording_elapsed_tick.get();
        live_elapsed_duration_ms(
            &live_recording.status.get(),
            live_recording.recording_started_at_ms.get(),
        )
    });
    let last_navigated_completion_nonce = RwSignal::new(None::<u64>);

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

    Effect::new(move |_| {
        let completion_nonce = transcription.completion_nonce.get();
        let job_status = transcription.job_status.get();
        let transcript = transcription.transcript.get();
        let last_navigated_nonce = last_navigated_completion_nonce.get();

        if should_navigate_to_live_transcript(
            completion_nonce,
            last_navigated_nonce,
            &job_status,
            transcript.as_ref(),
        ) {
            active_screen.set(Screen::Transcribe);
            last_navigated_completion_nonce.set(Some(completion_nonce));
        }
    });

    view! {
        <main class="shell">
            <HotkeyActivityBanner activity=hotkey_activity />
            <LiveRecordingBanner controller=live_recording elapsed_tick=recording_elapsed_tick />
            <div class="frame">
                <AppSidebar active=active_screen />

                <div class="screen" class:screen-active=move || active_screen.get() == Screen::Transcribe>
                    <TranscribeScreen
                        active=Signal::derive(move || active_screen.get() == Screen::Transcribe)
                        transcription=transcription
                        live_recording_state=live_recording_state
                        live_recording_label=live_recording_label
                        live_recording_elapsed_ms=live_recording_elapsed_ms
                    />
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

fn should_navigate_to_live_transcript(
    completion_nonce: u64,
    last_navigated_completion_nonce: Option<u64>,
    job_status: &crate::tauri_api::TranscriptionJobStatus,
    transcript: Option<&crate::tauri_api::TranscriptResult>,
) -> bool {
    last_navigated_completion_nonce != Some(completion_nonce)
        && matches!(
            job_status.state,
            crate::tauri_api::TranscriptionJobState::Succeeded
        )
        && matches!(job_status.input_type, InputType::Live)
        && transcript
            .map(|result| matches!(result.source.input_type, InputType::Live))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tauri_api::{
        TranscriptResult, TranscriptionJobState, TranscriptionJobStatus, TranscriptionSource,
    };

    #[test]
    fn should_navigate_to_live_transcript_only_once_per_live_completion() {
        let live_status = TranscriptionJobStatus {
            state: TranscriptionJobState::Succeeded,
            input_type: InputType::Live,
            source_name: Some("Desk Mic".to_string()),
            message: Some("Transcript ready for review.".to_string()),
        };
        let live_result = TranscriptResult {
            text: "hello".to_string(),
            segments: Vec::new(),
            source: TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                duration_ms: Some(1_000),
            },
            post_processed_text: None,
        };

        assert!(should_navigate_to_live_transcript(
            4,
            Some(3),
            &live_status,
            Some(&live_result),
        ));
        assert!(!should_navigate_to_live_transcript(
            4,
            Some(4),
            &live_status,
            Some(&live_result),
        ));
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
    let is_live_transcribing = Signal::derive(move || {
        controller.transcription.is_transcribing.get()
            && matches!(
                controller.transcription.job_status.get().input_type,
                InputType::Live
            )
    });

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
        } else if is_live_transcribing.get() {
            "Transcribing".to_string()
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

        if is_live_transcribing.get() {
            return controller
                .transcription
                .job_status
                .get()
                .message
                .or_else(|| controller.feedback_message.get())
                .unwrap_or_else(|| "Transcribing live recording...".to_string());
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
                    <Show when=move || is_live_transcribing.get()>
                        <span class="mini-chip">
                            {move || {
                                controller
                                    .transcription
                                    .progress_percent
                                    .get()
                                    .map(|progress| format!("Progress: {progress}%"))
                                    .unwrap_or_else(|| "Progress pending".to_string())
                            }}
                        </span>
                    </Show>
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
