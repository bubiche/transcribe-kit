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
                    message: Some("Receiving transcript segments from Whisper...".to_string()),
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
