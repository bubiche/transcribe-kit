use std::path::Path;

use reqwest::{multipart, StatusCode};
use serde::{Deserialize, Serialize};

use super::TranscriptionError;
use crate::models::{ChatMessage, InputType, TranscriptResult, TranscriptionSource};

pub const PROVIDER_ID: &str = "openai-compatible";
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
struct JsonResponse {
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

    parse_json_response(&response_text, normalized_model_name)
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
        .text("response_format", "json"))
}

fn parse_json_response(
    response_body: &str,
    model_name: &str,
) -> Result<TranscriptResult, TranscriptionError> {
    let parsed: JsonResponse = serde_json::from_str(response_body).map_err(|error| {
        TranscriptionError::ApiRequest(format!(
            "The transcription API returned an unexpected response format: {error}"
        ))
    })?;

    Ok(TranscriptResult {
        text: parsed.text.trim().to_string(),
        segments: Vec::new(),
        source: TranscriptionSource {
            provider: PROVIDER_ID.to_string(),
            model_id: model_name.to_string(),
            input_type: InputType::File,
            live_capture_profile: None,
            source_name: None,
            duration_ms: None,
        },
        post_processed_text: None,
    })
}

// ---- Chat completions (post-processing) ----

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: ChatChoiceMessage,
}

#[derive(Debug, Default, Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: Option<String>,
}

pub fn chat_completions_endpoint(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

pub async fn post_process_chat(
    messages: Vec<ChatMessage>,
    model: &str,
    credentials: &ApiCredentials,
) -> Result<String, TranscriptionError> {
    credentials
        .validate()
        .map_err(|message| TranscriptionError::ApiRequest(message.to_string()))?;

    let model = model.trim();
    if model.is_empty() {
        return Err(TranscriptionError::ApiRequest(
            "Post-processing model is required.".to_string(),
        ));
    }

    if messages.is_empty() {
        return Err(TranscriptionError::ApiRequest(
            "At least one chat message is required.".to_string(),
        ));
    }

    let request_body = ChatCompletionsRequest {
        model: model.to_string(),
        messages,
    };

    let response = reqwest::Client::new()
        .post(chat_completions_endpoint(&credentials.base_url))
        .bearer_auth(credentials.api_key.trim())
        .json(&request_body)
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
        return Err(TranscriptionError::ApiRequest(map_chat_completions_error(
            status,
            &response_text,
        )));
    }

    parse_chat_completions_response(&response_text)
}

fn parse_chat_completions_response(response_body: &str) -> Result<String, TranscriptionError> {
    let parsed: ChatCompletionsResponse = serde_json::from_str(response_body).map_err(|error| {
        TranscriptionError::ApiRequest(format!(
            "The post-processing API returned an unexpected response format: {error}"
        ))
    })?;

    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .unwrap_or_default();

    Ok(content.trim().to_string())
}

fn map_chat_completions_error(status: StatusCode, response_body: &str) -> String {
    let detail = extract_api_error_message(response_body);

    // Detect context-length errors on 400 responses
    if status == StatusCode::BAD_REQUEST {
        if let Some(ref detail_text) = detail {
            let lower = detail_text.to_ascii_lowercase();
            if lower.contains("context length")
                || lower.contains("too many tokens")
                || lower.contains("maximum context")
                || lower.contains("token limit")
            {
                return format!(
                    "The transcript may be too long for the selected model. \
                     Try a shorter transcript or a model with a larger context window. \
                     Details: {detail_text}"
                );
            }
        }
    }

    let base_message = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "Authentication failed. Check that your API key is valid for the configured provider."
                .to_string()
        }
        StatusCode::NOT_FOUND => {
            "The chat completions API endpoint was not found. Verify your API base URL.".to_string()
        }
        StatusCode::TOO_MANY_REQUESTS => {
            "The API rate limit has been reached. Please wait and try again.".to_string()
        }
        StatusCode::BAD_REQUEST => format!(
            "The post-processing API request was rejected (HTTP {}).",
            status.as_u16()
        ),
        _ if status.is_server_error() => format!(
            "The post-processing API returned a server error (HTTP {}). Please try again soon.",
            status.as_u16()
        ),
        _ => format!(
            "The post-processing API request failed with HTTP {}.",
            status.as_u16()
        ),
    };

    if let Some(detail) = detail {
        format!("{base_message} Details: {detail}")
    } else {
        base_message
    }
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
        build_multipart_upload_spec, chat_completions_endpoint, ensure_supported_audio_file_format,
        extract_api_error_message, infer_audio_mime_type, is_api_supported_audio_file,
        map_chat_completions_error, map_http_error, network_error_message,
        parse_chat_completions_response, parse_json_response, resolve_effective_model_name,
        transcription_endpoint, ApiCredentials, ChatMessage,
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
    fn parse_json_response_extracts_trimmed_text() {
        let response_body = r#"{ "text": " transcribed text here " }"#;

        let transcript =
            parse_json_response(response_body, "gpt-4o-mini-transcribe").expect("parse response");

        assert_eq!(transcript.text, "transcribed text here");
        assert!(transcript.segments.is_empty());
        assert_eq!(transcript.source.duration_ms, None);
        assert_eq!(transcript.source.model_id, "gpt-4o-mini-transcribe");
        assert_eq!(transcript.source.provider, "openai-compatible");
    }

    #[test]
    fn parse_json_response_handles_extra_fields_gracefully() {
        let response_body = r#"{
            "task": "transcribe",
            "language": "english",
            "duration": 8.47,
            "text": "hello",
            "segments": []
        }"#;

        let transcript = parse_json_response(response_body, "whisper-1").expect("parse response");
        assert_eq!(transcript.text, "hello");
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

    // ---- ApiCredentials::validate() ----

    #[test]
    fn api_credentials_rejects_empty_key() {
        let creds = ApiCredentials {
            api_key: "   ".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        };
        assert_eq!(creds.validate(), Err("API key is required."));
    }

    #[test]
    fn api_credentials_rejects_invalid_base_url_scheme() {
        let creds = ApiCredentials {
            api_key: "sk-test".to_string(),
            base_url: "ftp://api.openai.com/v1".to_string(),
        };
        assert_eq!(
            creds.validate(),
            Err("Base URL must start with http:// or https://")
        );
    }

    #[test]
    fn api_credentials_accepts_valid_configuration() {
        let https = ApiCredentials {
            api_key: "sk-test-key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        };
        assert_eq!(https.validate(), Ok(()));

        let http = ApiCredentials {
            api_key: "key".to_string(),
            base_url: "http://localhost:8080".to_string(),
        };
        assert_eq!(http.validate(), Ok(()));
    }

    // ---- transcription_endpoint() ----

    #[test]
    fn transcription_endpoint_appends_path_to_base_url() {
        assert_eq!(
            transcription_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/audio/transcriptions"
        );
    }

    #[test]
    fn transcription_endpoint_trims_trailing_slash() {
        assert_eq!(
            transcription_endpoint("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/audio/transcriptions"
        );
    }

    #[test]
    fn transcription_endpoint_works_with_custom_base_url() {
        assert_eq!(
            transcription_endpoint("http://localhost:8080/v1"),
            "http://localhost:8080/v1/audio/transcriptions"
        );
    }

    // ---- extract_api_error_message() ----

    #[test]
    fn extract_api_error_message_extracts_nested_error_message() {
        let body = r#"{"error":{"message":"Invalid API key provided"}}"#;
        assert_eq!(
            extract_api_error_message(body),
            Some("Invalid API key provided".to_string())
        );
    }

    #[test]
    fn extract_api_error_message_falls_back_to_top_level_message() {
        let body = r#"{"message":"Rate limit exceeded"}"#;
        assert_eq!(
            extract_api_error_message(body),
            Some("Rate limit exceeded".to_string())
        );
    }

    #[test]
    fn extract_api_error_message_returns_none_for_unparseable_body() {
        assert_eq!(extract_api_error_message("not json at all"), None);
        assert_eq!(extract_api_error_message(""), None);
    }

    #[test]
    fn extract_api_error_message_returns_none_for_empty_message_fields() {
        assert_eq!(
            extract_api_error_message(r#"{"error":{"message":""}}"#),
            None
        );
        assert_eq!(extract_api_error_message(r#"{"message":"  "}"#), None);
    }

    // ---- parse_json_response() error cases ----

    #[test]
    fn parse_json_response_rejects_malformed_json() {
        let error = parse_json_response("not valid json", "model")
            .expect_err("malformed json should fail")
            .to_string();
        assert!(error.contains("unexpected response format"));
    }

    #[test]
    fn parse_json_response_defaults_to_empty_text_when_field_missing() {
        let body = r#"{}"#;
        let result = parse_json_response(body, "model").expect("should parse with default");
        assert_eq!(result.text, "");
    }

    // ---- additional edge cases ----

    #[test]
    fn is_api_supported_audio_file_handles_uppercase_extensions() {
        assert!(is_api_supported_audio_file(Path::new("clip.WAV")));
        assert!(is_api_supported_audio_file(Path::new("clip.Mp3")));
        assert!(!is_api_supported_audio_file(Path::new("clip.FLAC")));
    }

    #[test]
    fn ensure_supported_format_shows_extension_in_flac_error() {
        let error = ensure_supported_audio_file_format(Path::new("recording.flac"))
            .expect_err("flac should fail")
            .to_string();
        assert!(error.contains(".flac"));
        assert!(error.contains("not supported"));
    }

    #[test]
    fn ensure_supported_format_handles_file_without_extension() {
        let error = ensure_supported_audio_file_format(Path::new("noextension"))
            .expect_err("no extension should fail")
            .to_string();
        assert!(error.contains("unknown file extension"));
    }

    #[test]
    fn infer_audio_mime_type_covers_all_supported_formats() {
        assert_eq!(infer_audio_mime_type(Path::new("a.wav")), "audio/wav");
        assert_eq!(infer_audio_mime_type(Path::new("a.mp3")), "audio/mpeg");
        assert_eq!(infer_audio_mime_type(Path::new("a.mpeg")), "audio/mpeg");
        assert_eq!(infer_audio_mime_type(Path::new("a.mpga")), "audio/mpeg");
        assert_eq!(infer_audio_mime_type(Path::new("a.m4a")), "audio/mp4");
        assert_eq!(infer_audio_mime_type(Path::new("a.mp4")), "audio/mp4");
        assert_eq!(infer_audio_mime_type(Path::new("a.webm")), "audio/webm");
        assert_eq!(
            infer_audio_mime_type(Path::new("a.ogg")),
            "application/octet-stream"
        );
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
    }

    // ---- chat completions (post-processing) ----

    #[test]
    fn chat_completions_endpoint_appends_path() {
        assert_eq!(
            chat_completions_endpoint("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_endpoint_trims_trailing_slash() {
        assert_eq!(
            chat_completions_endpoint("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_endpoint_works_with_localhost() {
        assert_eq!(
            chat_completions_endpoint("http://localhost:11434/v1"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn parse_chat_completions_response_extracts_content() {
        let body = r#"{
            "id": "chatcmpl-abc",
            "object": "chat.completion",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "  Here are the meeting notes.  "
                    },
                    "finish_reason": "stop"
                }
            ]
        }"#;

        let result = parse_chat_completions_response(body).expect("parse response");
        assert_eq!(result, "Here are the meeting notes.");
    }

    #[test]
    fn parse_chat_completions_response_handles_empty_choices() {
        let body = r#"{"choices": []}"#;
        let result = parse_chat_completions_response(body).expect("parse response");
        assert_eq!(result, "");
    }

    #[test]
    fn parse_chat_completions_response_handles_null_content() {
        let body = r#"{"choices": [{"message": {}}]}"#;
        let result = parse_chat_completions_response(body).expect("parse response");
        assert_eq!(result, "");
    }

    #[test]
    fn parse_chat_completions_response_handles_extra_fields() {
        let body = r#"{
            "id": "chatcmpl-xyz",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4o-mini",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Summary of transcript"
                    },
                    "logprobs": null,
                    "finish_reason": "stop"
                }
            ],
            "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150}
        }"#;

        let result = parse_chat_completions_response(body).expect("parse response");
        assert_eq!(result, "Summary of transcript");
    }

    #[test]
    fn parse_chat_completions_response_rejects_malformed_json() {
        let error = parse_chat_completions_response("not valid json")
            .expect_err("malformed json should fail")
            .to_string();
        assert!(error.contains("unexpected response format"));
    }

    #[test]
    fn map_chat_completions_error_detects_context_length() {
        let body =
            r#"{"error":{"message":"This model's maximum context length is 128000 tokens"}}"#;
        let error = map_chat_completions_error(StatusCode::BAD_REQUEST, body);
        assert!(error.contains("transcript may be too long"));
        assert!(error.contains("128000"));
    }

    #[test]
    fn map_chat_completions_error_detects_too_many_tokens() {
        let body = r#"{"error":{"message":"Too many tokens in the request"}}"#;
        let error = map_chat_completions_error(StatusCode::BAD_REQUEST, body);
        assert!(error.contains("transcript may be too long"));
    }

    #[test]
    fn map_chat_completions_error_generic_bad_request() {
        let body = r#"{"error":{"message":"Invalid request format"}}"#;
        let error = map_chat_completions_error(StatusCode::BAD_REQUEST, body);
        assert!(error.contains("rejected"));
        assert!(error.contains("Invalid request format"));
    }

    #[test]
    fn map_chat_completions_error_auth_failure() {
        let error = map_chat_completions_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Incorrect API key"}}"#,
        );
        assert!(error.contains("API key"));
        assert!(error.contains("Incorrect API key"));
    }

    #[test]
    fn map_chat_completions_error_not_found() {
        let error = map_chat_completions_error(StatusCode::NOT_FOUND, "");
        assert!(error.contains("chat completions"));
        assert!(error.contains("not found"));
    }

    #[test]
    fn map_chat_completions_error_rate_limit() {
        let error = map_chat_completions_error(StatusCode::TOO_MANY_REQUESTS, "");
        assert!(error.contains("rate limit"));
    }

    #[test]
    fn map_chat_completions_error_server_error() {
        let error = map_chat_completions_error(StatusCode::INTERNAL_SERVER_ERROR, "");
        assert!(error.contains("server error"));
        assert!(error.contains("500"));
    }

    // ---- post_process_chat validation ----

    fn sample_messages() -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: "user".to_string(),
            content: "prompt text".to_string(),
        }]
    }

    #[tokio::test]
    async fn post_process_chat_rejects_empty_model() {
        use super::post_process_chat;
        let credentials = ApiCredentials {
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        };
        let error = post_process_chat(sample_messages(), "   ", &credentials)
            .await
            .expect_err("empty model should fail");
        assert!(error.to_string().contains("model is required"));
    }

    #[tokio::test]
    async fn post_process_chat_rejects_empty_api_key() {
        use super::post_process_chat;
        let credentials = ApiCredentials {
            api_key: "   ".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
        };
        let error = post_process_chat(sample_messages(), "gpt-4o-mini", &credentials)
            .await
            .expect_err("empty key should fail");
        assert!(error.to_string().contains("API key"));
    }

    #[tokio::test]
    async fn post_process_chat_rejects_invalid_base_url() {
        use super::post_process_chat;
        let credentials = ApiCredentials {
            api_key: "sk-test".to_string(),
            base_url: "ftp://invalid".to_string(),
        };
        let error = post_process_chat(sample_messages(), "gpt-4o-mini", &credentials)
            .await
            .expect_err("invalid url should fail");
        assert!(error.to_string().contains("http"));
    }

    #[tokio::test]
    async fn post_process_chat_returns_network_error_for_unreachable_host() {
        use super::post_process_chat;
        let credentials = ApiCredentials {
            api_key: "sk-test".to_string(),
            base_url: "http://127.0.0.1:1".to_string(), // port 1 should be unreachable
        };
        let error = post_process_chat(sample_messages(), "gpt-4o-mini", &credentials)
            .await
            .expect_err("unreachable host should fail");
        let error_str = error.to_string();
        // Should be some kind of network/connection error
        assert!(
            error_str.contains("connect")
                || error_str.contains("Network")
                || error_str.contains("network"),
            "expected network error, got: {error_str}"
        );
    }
}
