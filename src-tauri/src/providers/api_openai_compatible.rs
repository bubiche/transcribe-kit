use std::path::Path;

use reqwest::{multipart, StatusCode};
use serde::Deserialize;

use super::TranscriptionError;
use crate::models::{InputType, TranscriptResult, TranscriptSegment, TranscriptionSource};

pub const PROVIDER_ID: &str = "openai-compatible";
const RESPONSE_FORMAT_VERBOSE_JSON: &str = "verbose_json";
const TIMESTAMP_GRANULARITY_SEGMENT: &str = "segment";
const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["mp3", "mp4", "mpeg", "mpga", "m4a", "wav", "webm"];

#[derive(Debug, Clone)]
pub struct ApiCredentials {
    pub api_key: String,
    pub base_url: String,
}

impl ApiCredentials {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.api_key.trim().is_empty() {
            return Err("API key is required.");
        }

        if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err("Base URL must start with http:// or https://");
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct VerboseJsonResponse {
    #[allow(dead_code)]
    #[serde(default)]
    task: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    segments: Vec<VerboseJsonSegment>,
}

#[derive(Debug, Deserialize)]
struct VerboseJsonSegment {
    #[allow(dead_code)]
    #[serde(default)]
    id: Option<i64>,
    start: f64,
    end: f64,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    #[serde(default)]
    error: Option<ApiErrorBody>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Clone)]
struct MultipartUploadSpec {
    file_name: String,
    mime_type: &'static str,
    model_name: String,
    file_bytes: Vec<u8>,
}

pub fn resolve_effective_model_name(
    api_model_id: &str,
    api_custom_model_name: &str,
) -> Result<String, &'static str> {
    let normalized_model_id = api_model_id.trim();
    if normalized_model_id.is_empty() {
        return Err("API model is required.");
    }

    if normalized_model_id == "custom" {
        let custom_name = api_custom_model_name.trim();
        if custom_name.is_empty() {
            return Err("Enter a model name for the custom API option.");
        }
        return Ok(custom_name.to_string());
    }

    Ok(normalized_model_id.to_string())
}

pub fn is_api_supported_audio_file(path: &Path) -> bool {
    normalized_audio_extension(path)
        .as_deref()
        .map(|extension| SUPPORTED_AUDIO_EXTENSIONS.contains(&extension))
        .unwrap_or(false)
}

pub fn ensure_supported_audio_file_format(path: &Path) -> Result<(), TranscriptionError> {
    if is_api_supported_audio_file(path) {
        return Ok(());
    }

    let extension_detail = normalized_audio_extension(path)
        .map(|extension| format!(".{extension}"))
        .unwrap_or_else(|| "unknown file extension".to_string());

    Err(TranscriptionError::ApiRequest(
        format!(
            "The selected audio format ({extension_detail}) is not supported for API transcription. Use mp3, mp4, mpeg, mpga, m4a, wav, or webm."
        ),
    ))
}

pub fn infer_audio_mime_type(path: &Path) -> &'static str {
    match normalized_audio_extension(path).as_deref() {
        Some("wav") => "audio/wav",
        Some("mp3" | "mpeg" | "mpga") => "audio/mpeg",
        Some("m4a" | "mp4") => "audio/mp4",
        Some("webm") => "audio/webm",
        _ => "application/octet-stream",
    }
}

pub fn transcription_endpoint(base_url: &str) -> String {
    format!("{}/audio/transcriptions", base_url.trim_end_matches('/'))
}

pub async fn transcribe_audio_file(
    file_path: &Path,
    model_name: &str,
    credentials: &ApiCredentials,
) -> Result<TranscriptResult, TranscriptionError> {
    credentials
        .validate()
        .map_err(|message| TranscriptionError::ApiRequest(message.to_string()))?;
    ensure_supported_audio_file_format(file_path)?;

    let normalized_model_name = model_name.trim();
    if normalized_model_name.is_empty() {
        return Err(TranscriptionError::ApiRequest(
            "API model is required.".to_string(),
        ));
    }

    let upload_spec = build_multipart_upload_spec(file_path, normalized_model_name)?;
    let form = build_multipart_form(upload_spec)?;

    let response = reqwest::Client::new()
        .post(transcription_endpoint(&credentials.base_url))
        .bearer_auth(credentials.api_key.trim())
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            TranscriptionError::ApiRequest(network_error_message(
                error.is_timeout(),
                error.is_connect(),
                &error.to_string(),
            ))
        })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|error| {
        TranscriptionError::ApiRequest(format!("Could not read the API response body: {error}"))
    })?;

    if !status.is_success() {
        return Err(TranscriptionError::ApiRequest(map_http_error(
            status,
            &response_text,
        )));
    }

    parse_verbose_json_response(&response_text, normalized_model_name)
}

fn build_multipart_upload_spec(
    file_path: &Path,
    model_name: &str,
) -> Result<MultipartUploadSpec, TranscriptionError> {
    let file_bytes = std::fs::read(file_path).map_err(|error| {
        TranscriptionError::ApiRequest(format!(
            "Could not read the selected audio file for upload: {error}"
        ))
    })?;

    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio")
        .to_string();

    Ok(MultipartUploadSpec {
        file_name,
        mime_type: infer_audio_mime_type(file_path),
        model_name: model_name.to_string(),
        file_bytes,
    })
}

fn build_multipart_form(spec: MultipartUploadSpec) -> Result<multipart::Form, TranscriptionError> {
    let file_part = multipart::Part::bytes(spec.file_bytes)
        .file_name(spec.file_name)
        .mime_str(spec.mime_type)
        .map_err(|error| {
            TranscriptionError::ApiRequest(format!("Could not prepare audio upload body: {error}"))
        })?;

    Ok(multipart::Form::new()
        .part("file", file_part)
        .text("model", spec.model_name)
        .text("response_format", RESPONSE_FORMAT_VERBOSE_JSON)
        .text("timestamp_granularities[]", TIMESTAMP_GRANULARITY_SEGMENT))
}

fn parse_verbose_json_response(
    response_body: &str,
    model_name: &str,
) -> Result<TranscriptResult, TranscriptionError> {
    let parsed: VerboseJsonResponse = serde_json::from_str(response_body).map_err(|error| {
        TranscriptionError::ApiRequest(format!(
            "The transcription API returned an unexpected response format: {error}"
        ))
    })?;

    verbose_json_to_transcript_result(parsed, model_name)
}

fn verbose_json_to_transcript_result(
    response: VerboseJsonResponse,
    model_name: &str,
) -> Result<TranscriptResult, TranscriptionError> {
    let mut segments = Vec::with_capacity(response.segments.len());
    for (index, segment) in response.segments.into_iter().enumerate() {
        segments.push(api_segment_to_transcript_segment(segment, index)?);
    }

    Ok(TranscriptResult {
        text: response.text.trim().to_string(),
        segments,
        source: TranscriptionSource {
            provider: PROVIDER_ID.to_string(),
            model_id: model_name.to_string(),
            input_type: InputType::File,
            live_capture_profile: None,
            source_name: None,
            duration_ms: response.duration.and_then(seconds_to_duration_ms),
        },
        post_processed_text: None,
    })
}

fn api_segment_to_transcript_segment(
    segment: VerboseJsonSegment,
    index: usize,
) -> Result<TranscriptSegment, TranscriptionError> {
    let start_ms = seconds_to_milliseconds(segment.start).ok_or_else(|| {
        TranscriptionError::ApiRequest(format!(
            "The transcription API returned an invalid start timestamp for segment {}.",
            index + 1
        ))
    })?;
    let end_ms = seconds_to_milliseconds(segment.end).ok_or_else(|| {
        TranscriptionError::ApiRequest(format!(
            "The transcription API returned an invalid end timestamp for segment {}.",
            index + 1
        ))
    })?;

    if end_ms < start_ms {
        return Err(TranscriptionError::ApiRequest(format!(
            "The transcription API returned segment {} with end time earlier than start time.",
            index + 1
        )));
    }

    Ok(TranscriptSegment {
        start_ms,
        end_ms,
        text: segment.text.trim().to_string(),
    })
}

fn seconds_to_milliseconds(seconds: f64) -> Option<i64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    let value = (seconds * 1_000.0).round();
    if value > i64::MAX as f64 {
        return Some(i64::MAX);
    }

    Some(value as i64)
}

fn seconds_to_duration_ms(seconds: f64) -> Option<u64> {
    seconds_to_milliseconds(seconds).and_then(|value| u64::try_from(value).ok())
}

fn normalized_audio_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
}

fn map_http_error(status: StatusCode, response_body: &str) -> String {
    let detail = extract_api_error_message(response_body);

    let base_message = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "Authentication failed. Check that your API key is valid for the configured provider."
                .to_string()
        }
        StatusCode::PAYLOAD_TOO_LARGE => {
            "The selected audio file is too large for the transcription API request.".to_string()
        }
        StatusCode::NOT_FOUND => {
            "The transcription API endpoint was not found. Verify your API base URL.".to_string()
        }
        StatusCode::TOO_MANY_REQUESTS => {
            "The transcription API rate limit has been reached. Please wait and try again."
                .to_string()
        }
        _ if status.is_server_error() => format!(
            "The transcription API returned a server error (HTTP {}). Please try again soon.",
            status.as_u16()
        ),
        _ => format!(
            "The transcription API request failed with HTTP {}.",
            status.as_u16()
        ),
    };

    if let Some(detail) = detail {
        format!("{base_message} Details: {detail}")
    } else {
        base_message
    }
}

fn extract_api_error_message(response_body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<ApiErrorEnvelope>(response_body).ok()?;
    parsed
        .error
        .and_then(|error| error.message)
        .or(parsed.message)
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
}

fn network_error_message(is_timeout: bool, is_connect: bool, details: &str) -> String {
    if is_timeout {
        return "The transcription API request timed out. Check your network and try again."
            .to_string();
    }

    if is_connect {
        return "Transcribe Kit could not connect to the transcription API. Verify your base URL and network connection."
            .to_string();
    }

    format!("Network error while calling the transcription API: {details}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use reqwest::StatusCode;
    use tempfile::TempDir;

    use super::{
        build_multipart_upload_spec, ensure_supported_audio_file_format, infer_audio_mime_type,
        is_api_supported_audio_file, map_http_error, network_error_message,
        parse_verbose_json_response, resolve_effective_model_name, RESPONSE_FORMAT_VERBOSE_JSON,
        TIMESTAMP_GRANULARITY_SEGMENT,
    };

    #[test]
    fn resolve_effective_model_name_returns_selected_id_when_not_custom() {
        let resolved =
            resolve_effective_model_name("gpt-4o-mini-transcribe", "").expect("resolved model");
        assert_eq!(resolved, "gpt-4o-mini-transcribe");
    }

    #[test]
    fn resolve_effective_model_name_uses_custom_name() {
        let resolved =
            resolve_effective_model_name("custom", " custom-model-v1 ").expect("resolved model");
        assert_eq!(resolved, "custom-model-v1");
    }

    #[test]
    fn resolve_effective_model_name_rejects_empty_custom_name() {
        let error = resolve_effective_model_name("custom", "   ").expect_err("empty custom name");
        assert_eq!(error, "Enter a model name for the custom API option.");
    }

    #[test]
    fn resolve_effective_model_name_rejects_empty_model_id() {
        let error = resolve_effective_model_name("   ", "ignored").expect_err("empty model id");
        assert_eq!(error, "API model is required.");
    }

    #[test]
    fn infer_audio_mime_type_matches_supported_extensions() {
        assert_eq!(infer_audio_mime_type(Path::new("note.wav")), "audio/wav");
        assert_eq!(infer_audio_mime_type(Path::new("note.mp3")), "audio/mpeg");
        assert_eq!(infer_audio_mime_type(Path::new("note.m4a")), "audio/mp4");
        assert_eq!(
            infer_audio_mime_type(Path::new("note.unknown")),
            "application/octet-stream"
        );
    }

    #[test]
    fn supported_audio_format_checks_accept_and_reject_expected_extensions() {
        assert!(is_api_supported_audio_file(Path::new("clip.wav")));
        assert!(is_api_supported_audio_file(Path::new("clip.mp3")));
        assert!(is_api_supported_audio_file(Path::new("clip.m4a")));
        assert!(is_api_supported_audio_file(Path::new("clip.mp4")));
        assert!(is_api_supported_audio_file(Path::new("clip.webm")));
        assert!(!is_api_supported_audio_file(Path::new("clip.flac")));
        assert!(!is_api_supported_audio_file(Path::new("clip.ogg")));

        let error = ensure_supported_audio_file_format(Path::new("clip.ogg"))
            .expect_err("unsupported extension should fail")
            .to_string();
        assert!(error.contains("not supported"));
        assert!(error.contains(".ogg"));
    }

    #[test]
    fn parse_verbose_json_response_maps_segments_and_duration_to_milliseconds() {
        let response_body = r#"{
            "task": "transcribe",
            "language": "english",
            "duration": 8.47,
            "text": "transcribed text here",
            "segments": [
                { "id": 0, "start": 0.01, "end": 3.32, "text": "first" },
                { "id": 1, "start": 3.32, "end": 8.47, "text": "second" }
            ]
        }"#;

        let transcript = parse_verbose_json_response(response_body, "gpt-4o-mini-transcribe")
            .expect("parse response");

        assert_eq!(transcript.text, "transcribed text here");
        assert_eq!(transcript.source.duration_ms, Some(8_470));
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[0].start_ms, 10);
        assert_eq!(transcript.segments[0].end_ms, 3_320);
        assert_eq!(transcript.segments[1].start_ms, 3_320);
        assert_eq!(transcript.segments[1].end_ms, 8_470);
    }

    #[test]
    fn parse_verbose_json_response_handles_empty_segments() {
        let response_body = r#"{
            "duration": 2.0,
            "text": "hello",
            "segments": []
        }"#;

        let transcript = parse_verbose_json_response(response_body, "gpt-4o-mini-transcribe")
            .expect("parse response");
        assert!(transcript.segments.is_empty());
        assert_eq!(transcript.source.duration_ms, Some(2_000));
    }

    #[test]
    fn parse_verbose_json_response_rejects_invalid_segment_range() {
        let response_body = r#"{
            "duration": 2.0,
            "text": "hello",
            "segments": [
                { "id": 0, "start": 1.5, "end": 1.0, "text": "bad range" }
            ]
        }"#;

        let error = parse_verbose_json_response(response_body, "gpt-4o-mini-transcribe")
            .expect_err("invalid segment range should fail")
            .to_string();

        assert!(error.contains("end time earlier than start time"));
    }

    #[test]
    fn parse_verbose_json_response_rejects_negative_segment_timestamp() {
        let response_body = r#"{
            "duration": 2.0,
            "text": "hello",
            "segments": [
                { "id": 0, "start": -0.1, "end": 0.8, "text": "bad start" }
            ]
        }"#;

        let error = parse_verbose_json_response(response_body, "gpt-4o-mini-transcribe")
            .expect_err("negative timestamp should fail")
            .to_string();

        assert!(error.contains("invalid start timestamp"));
    }

    #[test]
    fn map_http_error_uses_clear_messages_for_common_status_codes() {
        let unauthorized = map_http_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"invalid key"}}"#,
        );
        assert!(unauthorized.contains("API key"));
        assert!(unauthorized.contains("invalid key"));

        let forbidden = map_http_error(StatusCode::FORBIDDEN, "");
        assert!(forbidden.contains("Authentication failed"));

        let payload_too_large = map_http_error(StatusCode::PAYLOAD_TOO_LARGE, "");
        assert!(payload_too_large.contains("too large"));

        let not_found = map_http_error(StatusCode::NOT_FOUND, "");
        assert!(not_found.contains("endpoint was not found"));

        let rate_limited = map_http_error(StatusCode::TOO_MANY_REQUESTS, "");
        assert!(rate_limited.contains("rate limit"));

        let server_error = map_http_error(StatusCode::INTERNAL_SERVER_ERROR, "");
        assert!(server_error.contains("server error"));
    }

    #[test]
    fn network_error_messages_are_user_friendly() {
        let timeout = network_error_message(true, false, "timeout");
        assert!(timeout.contains("timed out"));

        let connect = network_error_message(false, true, "connect");
        assert!(connect.contains("could not connect"));

        let generic = network_error_message(false, false, "io error");
        assert!(generic.contains("Network error"));
    }

    #[test]
    fn multipart_upload_spec_contains_expected_request_fields() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("sample.wav");
        std::fs::write(&file_path, b"RIFF\0\0\0\0WAVE").expect("write sample wav");

        let spec = build_multipart_upload_spec(&file_path, "gpt-4o-mini-transcribe").expect("spec");

        assert_eq!(spec.file_name, "sample.wav");
        assert_eq!(spec.model_name, "gpt-4o-mini-transcribe");
        assert_eq!(spec.mime_type, "audio/wav");
        assert_eq!(spec.file_bytes, b"RIFF\0\0\0\0WAVE");
        assert_eq!(RESPONSE_FORMAT_VERBOSE_JSON, "verbose_json");
        assert_eq!(TIMESTAMP_GRANULARITY_SEGMENT, "segment");
    }
}
