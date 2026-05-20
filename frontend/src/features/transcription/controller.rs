use leptos::prelude::*;

use crate::tauri_api::{
    InputType, TranscriptResult, TranscriptSegment, TranscriptionJobState, TranscriptionJobStatus,
    TranscriptionStreamEvent,
};

#[derive(Clone, Copy)]
pub struct TranscriptionController {
    pub transcript: RwSignal<Option<TranscriptResult>>,
    pub partial_text: RwSignal<String>,
    pub partial_segments: RwSignal<Vec<TranscriptSegment>>,
    pub progress_percent: RwSignal<Option<i32>>,
    pub job_status: RwSignal<TranscriptionJobStatus>,
    pub is_transcribing: RwSignal<bool>,
    pub completion_nonce: RwSignal<u64>,
}

impl TranscriptionController {
    pub fn new() -> Self {
        Self {
            transcript: RwSignal::new(None),
            partial_text: RwSignal::new(String::new()),
            partial_segments: RwSignal::new(Vec::new()),
            progress_percent: RwSignal::new(None),
            job_status: RwSignal::new(idle_job_status(InputType::File)),
            is_transcribing: RwSignal::new(false),
            completion_nonce: RwSignal::new(0),
        }
    }

    pub fn reset_job_feedback(self) {
        self.job_status.update(|status| {
            status.message = None;
            if !matches!(status.state, TranscriptionJobState::Succeeded) {
                status.state = TranscriptionJobState::Idle;
            }
        });
    }

    pub fn set_preflight_failure(self, input_type: InputType, message: impl Into<String>) {
        self.clear_partial_state();
        self.is_transcribing.set(false);
        self.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Failed,
            input_type,
            source_name: None,
            message: Some(message.into()),
        });
    }

    pub fn start_file_job(self, source_name: impl Into<String>) {
        self.start_job(InputType::File, source_name.into());
    }

    #[allow(dead_code)]
    pub fn start_live_job(self, source_name: impl Into<String>) {
        self.start_job(InputType::Live, source_name.into());
    }

    pub fn apply_stream_event(self, event: TranscriptionStreamEvent) {
        let input_type = self.job_status.get_untracked().input_type;
        let source_name = self.job_status.get_untracked().source_name.clone();

        match event {
            TranscriptionStreamEvent::Progress { progress_percent } => {
                self.progress_percent.set(Some(progress_percent));
                self.job_status.set(TranscriptionJobStatus {
                    state: TranscriptionJobState::Running,
                    input_type,
                    source_name: source_name.clone(),
                    message: Some(progress_message(&source_name, progress_percent)),
                });
            }
            TranscriptionStreamEvent::Segment {
                segment_index,
                segment,
                accumulated_text,
            } => {
                self.partial_text.set(accumulated_text);
                self.partial_segments.update(|segments| {
                    let index = segment_index.max(0) as usize;
                    if index < segments.len() {
                        segments[index] = segment;
                    } else {
                        segments.push(segment);
                    }
                });
                self.job_status.set(TranscriptionJobStatus {
                    state: TranscriptionJobState::Running,
                    input_type,
                    source_name,
                    message: Some("Receiving transcript segments...".to_string()),
                });
            }
        }
    }

    pub fn complete_job(self, result: TranscriptResult) {
        self.transcript.set(Some(result.clone()));
        self.partial_text.set(result.text.clone());
        self.partial_segments.set(result.segments.clone());
        self.progress_percent.set(Some(100));
        self.is_transcribing.set(false);
        self.completion_nonce
            .update(|nonce| *nonce = nonce.saturating_add(1));
        self.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Succeeded,
            input_type: result.source.input_type.clone(),
            source_name: result.source.source_name.clone(),
            message: Some("Transcript ready for review.".to_string()),
        });
    }

    pub fn fail_job(
        self,
        input_type: InputType,
        source_name: Option<String>,
        error: impl Into<String>,
    ) {
        self.transcript.set(None);
        self.progress_percent.set(None);
        self.is_transcribing.set(false);
        self.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Failed,
            input_type,
            source_name,
            message: Some(error.into()),
        });
    }

    fn start_job(self, input_type: InputType, source_name: String) {
        self.transcript.set(None);
        self.clear_partial_state();
        self.progress_percent.set(Some(0));
        self.is_transcribing.set(true);
        self.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Running,
            input_type,
            source_name: Some(source_name.clone()),
            message: Some(start_message(&source_name)),
        });
    }

    fn clear_partial_state(self) {
        self.partial_text.set(String::new());
        self.partial_segments.set(Vec::new());
        self.progress_percent.set(None);
    }
}

fn idle_job_status(input_type: InputType) -> TranscriptionJobStatus {
    TranscriptionJobStatus {
        state: TranscriptionJobState::Idle,
        input_type,
        source_name: None,
        message: None,
    }
}

fn progress_message(source_name: &Option<String>, progress_percent: i32) -> String {
    match source_name {
        Some(source_name) if !source_name.trim().is_empty() => {
            format!("Transcribing {source_name} ({progress_percent}%)")
        }
        _ => format!("Transcribing audio ({progress_percent}%)"),
    }
}

fn start_message(source_name: &str) -> String {
    if source_name.trim().is_empty() {
        "Transcribing audio".to_string()
    } else {
        format!("Transcribing {source_name}")
    }
}

pub fn auto_save_transcription_note(result: &TranscriptResult) {
    let text = result.text.clone();
    let source_name = result.source.source_name.clone();
    let input_type = result.source.input_type.clone();

    leptos::task::spawn_local(async move {
        let date = js_sys::Date::new_0();
        let date_str = format!(
            "{}-{:02}-{:02}",
            date.get_full_year(),
            date.get_month() + 1,
            date.get_date(),
        );
        let source_label = source_name.unwrap_or_else(|| match input_type {
            InputType::Live => "Live recording".to_string(),
            InputType::File => "Unknown file".to_string(),
        });
        let title = format!("Transcription - {} - {}", source_label, date_str);

        if let Err(err) =
            crate::tauri_api::create_note(title, text, crate::tauri_api::NoteSource::Transcription)
                .await
        {
            web_sys::console::warn_1(
                &format!("Failed to auto-save transcription note: {err}").into(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tauri_api::TranscriptionSource;

    #[test]
    fn start_live_job_clears_previous_result_and_enters_running_state() {
        let controller = TranscriptionController::new();
        controller.transcript.set(Some(sample_result(
            InputType::File,
            Some("old-note.wav"),
            "old text",
            vec![sample_segment(0, 800, "old text")],
        )));
        controller.partial_text.set("stale partial".to_string());
        controller
            .partial_segments
            .set(vec![sample_segment(0, 400, "stale partial")]);
        controller.progress_percent.set(Some(87));

        controller.start_live_job("Desk Mic");

        assert_eq!(controller.transcript.get(), None);
        assert_eq!(controller.partial_text.get(), "");
        assert!(controller.partial_segments.get().is_empty());
        assert_eq!(controller.progress_percent.get(), Some(0));
        assert!(controller.is_transcribing.get());
        assert_eq!(
            controller.job_status.get(),
            TranscriptionJobStatus {
                state: TranscriptionJobState::Running,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("Transcribing Desk Mic".to_string()),
            }
        );
    }

    #[test]
    fn apply_stream_event_updates_live_progress_and_replaces_segment_by_index() {
        let controller = TranscriptionController::new();
        controller.start_live_job("Desk Mic");

        controller.apply_stream_event(TranscriptionStreamEvent::Progress {
            progress_percent: 42,
        });
        assert_eq!(controller.progress_percent.get(), Some(42));
        assert_eq!(
            controller.job_status.get().message.as_deref(),
            Some("Transcribing Desk Mic (42%)")
        );

        controller.apply_stream_event(TranscriptionStreamEvent::Segment {
            segment_index: 0,
            segment: sample_segment(0, 900, "hello"),
            accumulated_text: "hello".to_string(),
        });
        controller.apply_stream_event(TranscriptionStreamEvent::Segment {
            segment_index: 0,
            segment: sample_segment(0, 1_100, "hello there"),
            accumulated_text: "hello there".to_string(),
        });

        assert_eq!(controller.partial_text.get(), "hello there");
        assert_eq!(
            controller.partial_segments.get(),
            vec![sample_segment(0, 1_100, "hello there")]
        );
        assert_eq!(
            controller.job_status.get().message.as_deref(),
            Some("Receiving transcript segments...")
        );
        assert_eq!(controller.job_status.get().input_type, InputType::Live);
    }

    #[test]
    fn complete_job_promotes_live_result_into_review_state() {
        let controller = TranscriptionController::new();
        controller.start_live_job("Desk Mic");
        controller.partial_text.set("draft".to_string());
        controller
            .partial_segments
            .set(vec![sample_segment(0, 300, "draft")]);

        let result = sample_result(
            InputType::Live,
            Some("Desk Mic"),
            "final transcript",
            vec![
                sample_segment(0, 500, "final"),
                sample_segment(500, 1_000, "transcript"),
            ],
        );

        controller.complete_job(result.clone());

        assert_eq!(controller.transcript.get(), Some(result));
        assert_eq!(controller.partial_text.get(), "final transcript");
        assert_eq!(
            controller.partial_segments.get(),
            vec![
                sample_segment(0, 500, "final"),
                sample_segment(500, 1_000, "transcript"),
            ]
        );
        assert_eq!(controller.progress_percent.get(), Some(100));
        assert!(!controller.is_transcribing.get());
        assert_eq!(controller.completion_nonce.get(), 1);
        assert_eq!(
            controller.job_status.get(),
            TranscriptionJobStatus {
                state: TranscriptionJobState::Succeeded,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("Transcript ready for review.".to_string()),
            }
        );
    }

    #[test]
    fn fail_job_clears_stale_transcript_and_preserves_partial_draft() {
        let controller = TranscriptionController::new();
        controller.transcript.set(Some(sample_result(
            InputType::File,
            Some("old-note.wav"),
            "old text",
            vec![sample_segment(0, 800, "old text")],
        )));
        controller
            .partial_text
            .set("partial live draft".to_string());
        controller
            .partial_segments
            .set(vec![sample_segment(0, 700, "partial live draft")]);
        controller.progress_percent.set(Some(63));
        controller.is_transcribing.set(true);

        controller.fail_job(
            InputType::Live,
            Some("Desk Mic".to_string()),
            "Whisper failed",
        );

        assert_eq!(controller.transcript.get(), None);
        assert_eq!(controller.partial_text.get(), "partial live draft");
        assert_eq!(
            controller.partial_segments.get(),
            vec![sample_segment(0, 700, "partial live draft")]
        );
        assert_eq!(controller.progress_percent.get(), None);
        assert!(!controller.is_transcribing.get());
        assert_eq!(
            controller.job_status.get(),
            TranscriptionJobStatus {
                state: TranscriptionJobState::Failed,
                input_type: InputType::Live,
                source_name: Some("Desk Mic".to_string()),
                message: Some("Whisper failed".to_string()),
            }
        );
    }

    #[test]
    fn set_preflight_failure_clears_partial_state() {
        let controller = TranscriptionController::new();
        controller.partial_text.set("stale".to_string());
        controller
            .partial_segments
            .set(vec![sample_segment(0, 500, "stale")]);
        controller.progress_percent.set(Some(10));
        controller.is_transcribing.set(true);

        controller.set_preflight_failure(InputType::Live, "Provider mismatch");

        assert_eq!(controller.partial_text.get(), "");
        assert!(controller.partial_segments.get().is_empty());
        assert_eq!(controller.progress_percent.get(), None);
        assert!(!controller.is_transcribing.get());
        assert_eq!(
            controller.job_status.get(),
            TranscriptionJobStatus {
                state: TranscriptionJobState::Failed,
                input_type: InputType::Live,
                source_name: None,
                message: Some("Provider mismatch".to_string()),
            }
        );
    }

    #[test]
    fn reset_job_feedback_preserves_succeeded_state() {
        let controller = TranscriptionController::new();
        controller.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Succeeded,
            input_type: InputType::File,
            source_name: Some("note.wav".to_string()),
            message: Some("Transcript ready for review.".to_string()),
        });

        controller.reset_job_feedback();

        assert_eq!(
            controller.job_status.get().state,
            TranscriptionJobState::Succeeded
        );
        assert_eq!(controller.job_status.get().message, None);
    }

    #[test]
    fn reset_job_feedback_resets_failed_state_to_idle() {
        let controller = TranscriptionController::new();
        controller.job_status.set(TranscriptionJobStatus {
            state: TranscriptionJobState::Failed,
            input_type: InputType::File,
            source_name: None,
            message: Some("API key missing".to_string()),
        });

        controller.reset_job_feedback();

        assert_eq!(
            controller.job_status.get().state,
            TranscriptionJobState::Idle
        );
        assert_eq!(controller.job_status.get().message, None);
    }

    #[test]
    fn completion_nonce_increments_across_successive_jobs() {
        let controller = TranscriptionController::new();
        assert_eq!(controller.completion_nonce.get(), 0);

        controller.complete_job(sample_result(
            InputType::File,
            Some("a.wav"),
            "first",
            vec![],
        ));
        assert_eq!(controller.completion_nonce.get(), 1);

        controller.complete_job(sample_result(
            InputType::Live,
            Some("Mic"),
            "second",
            vec![],
        ));
        assert_eq!(controller.completion_nonce.get(), 2);
    }

    #[test]
    fn complete_job_sets_api_provider_metadata_correctly() {
        let controller = TranscriptionController::new();
        controller.start_file_job("clip.mp3");

        let result = TranscriptResult {
            text: "api transcript".to_string(),
            segments: vec![],
            source: TranscriptionSource {
                provider: "openai-compatible".to_string(),
                model_id: "gpt-4o-mini-transcribe".to_string(),
                input_type: InputType::File,
                live_capture_profile: None,
                source_name: Some("clip.mp3".to_string()),
                duration_ms: Some(5_000),
            },
            post_processed_text: None,
        };

        controller.complete_job(result);

        let transcript = controller.transcript.get().expect("should have transcript");
        assert_eq!(transcript.source.provider, "openai-compatible");
        assert_eq!(transcript.source.model_id, "gpt-4o-mini-transcribe");
        assert_eq!(transcript.text, "api transcript");
        assert!(transcript.segments.is_empty());
        assert!(!controller.is_transcribing.get());
    }

    fn sample_result(
        input_type: InputType,
        source_name: Option<&str>,
        text: &str,
        segments: Vec<TranscriptSegment>,
    ) -> TranscriptResult {
        TranscriptResult {
            text: text.to_string(),
            segments,
            source: TranscriptionSource {
                provider: "whisper".to_string(),
                model_id: "whisper-base".to_string(),
                input_type,
                live_capture_profile: None,
                source_name: source_name.map(str::to_string),
                duration_ms: Some(1_000),
            },
            post_processed_text: None,
        }
    }

    fn sample_segment(start_ms: i64, end_ms: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
        }
    }
}
