use crate::tauri_api::TranscriptSegment;

pub fn file_name_from_path(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub fn format_duration_label(duration_ms: u64) -> String {
    format!("Duration: {}", format_timestamp(duration_ms as i64))
}

pub fn format_transcript_with_timestamps(
    segments: &[TranscriptSegment],
    fallback_text: &str,
) -> String {
    if segments.is_empty() {
        return fallback_text.trim().to_string();
    }

    segments
        .iter()
        .map(|segment| {
            format!(
                "[{} - {}] {}",
                format_timestamp(segment.start_ms),
                format_timestamp(segment.end_ms),
                segment.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_timestamp(milliseconds: i64) -> String {
    let total_seconds = (milliseconds.max(0) / 1000) as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_transcript_with_timestamps_falls_back_to_plain_text_without_segments() {
        let formatted = format_transcript_with_timestamps(&[], "  Hello world  ");

        assert_eq!(formatted, "Hello world");
    }

    #[test]
    fn format_transcript_with_timestamps_renders_timestamped_lines() {
        let segments = vec![
            TranscriptSegment {
                start_ms: 0,
                end_ms: 3_000,
                text: " Hello ".to_string(),
            },
            TranscriptSegment {
                start_ms: 3_000,
                end_ms: 7_000,
                text: "world".to_string(),
            },
        ];

        let formatted = format_transcript_with_timestamps(&segments, "ignored");

        assert_eq!(formatted, "[00:00 - 00:03] Hello\n[00:03 - 00:07] world");
    }
}
