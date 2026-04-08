use std::fs::File;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;
use std::mem::MaybeUninit;
use std::path::Path;

use ogg::PacketReader;
use opus_decoder::OpusDecoder;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::providers::TranscriptionError;

const WHISPER_SAMPLE_RATE: u32 = 16_000;

pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub duration_ms: Option<u64>,
}

pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio, TranscriptionError> {
    if is_ogg_path(path) {
        match decode_ogg_opus_file(path) {
            Ok(decoded) => return Ok(decoded),
            Err(opus_error) => match decode_with_symphonia(path) {
                Ok(decoded) => return Ok(decoded),
                Err(symphonia_error) => {
                    return Err(TranscriptionError::AudioDecode(format!(
                        "{opus_error}. Symphonia fallback also failed: {symphonia_error}"
                    )));
                }
            },
        }
    }

    decode_with_symphonia(path)
}

const MP3_ENCODE_SAMPLE_RATE: u32 = 16_000;

/// Decodes an audio file to 16 kHz mono PCM, then encodes it as a 64 kbps MP3.
///
/// Used to compress large audio files before uploading to the transcription API,
/// which has a 25 MB upload limit.
pub fn encode_mp3_for_upload(
    source_path: &Path,
    output_path: &Path,
) -> Result<(), TranscriptionError> {
    let decoded = decode_audio_file(source_path)?;

    let i16_samples: Vec<i16> = decoded
        .samples
        .iter()
        .map(|&s| (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
        .collect();

    let mut builder = mp3lame_encoder::Builder::new()
        .ok_or_else(|| TranscriptionError::AudioEncode("MP3 encoder init failed".to_string()))?;
    builder.set_num_channels(1).map_err(|e| {
        TranscriptionError::AudioEncode(format!("MP3 encoder config failed: {e:?}"))
    })?;
    builder
        .set_sample_rate(MP3_ENCODE_SAMPLE_RATE)
        .map_err(|e| {
            TranscriptionError::AudioEncode(format!("MP3 encoder config failed: {e:?}"))
        })?;
    builder
        .set_brate(mp3lame_encoder::Bitrate::Kbps64)
        .map_err(|e| {
            TranscriptionError::AudioEncode(format!("MP3 encoder config failed: {e:?}"))
        })?;
    builder
        .set_quality(mp3lame_encoder::Quality::Best)
        .map_err(|e| {
            TranscriptionError::AudioEncode(format!("MP3 encoder config failed: {e:?}"))
        })?;

    let mut encoder = builder
        .build()
        .map_err(|e| TranscriptionError::AudioEncode(format!("MP3 encoder build failed: {e:?}")))?;

    let buf_size = mp3lame_encoder::max_required_buffer_size(i16_samples.len());
    let mut mp3_buffer: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); buf_size];

    let encoded_size = encoder
        .encode(mp3lame_encoder::MonoPcm(&i16_samples), &mut mp3_buffer)
        .map_err(|e| TranscriptionError::AudioEncode(format!("MP3 encoding failed: {e:?}")))?;

    let flush_size = encoder
        .flush::<mp3lame_encoder::FlushNoGap>(&mut mp3_buffer[encoded_size..])
        .map_err(|e| TranscriptionError::AudioEncode(format!("MP3 flush failed: {e:?}")))?;

    let total_size = encoded_size + flush_size;
    // SAFETY: encode() and flush() initialized exactly `total_size` bytes.
    let mp3_bytes =
        unsafe { std::slice::from_raw_parts(mp3_buffer.as_ptr() as *const u8, total_size) };

    let mut file = File::create(output_path).map_err(|e| {
        TranscriptionError::AudioEncode(format!("Could not create compressed audio file: {e}"))
    })?;
    file.write_all(mp3_bytes).map_err(|e| {
        TranscriptionError::AudioEncode(format!("Could not write compressed audio file: {e}"))
    })?;

    Ok(())
}

fn decode_with_symphonia(path: &Path) -> Result<DecodedAudio, TranscriptionError> {
    let file = File::open(path)
        .map_err(|e| TranscriptionError::AudioDecode(format!("Cannot open audio file: {e}")))?;

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }

    let media_source_stream =
        MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let probed = get_probe()
        .format(
            &hint,
            media_source_stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| TranscriptionError::AudioDecode(format!("Unsupported audio file: {e}")))?;

    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| {
        TranscriptionError::AudioDecode("No default audio track found in file".to_string())
    })?;
    let track_id = track.id;

    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| TranscriptionError::AudioDecode(format!("Cannot decode audio stream: {e}")))?;

    let mut interleaved_samples = Vec::<f32>::new();
    let mut sample_rate = track
        .codec_params
        .sample_rate
        .unwrap_or(WHISPER_SAMPLE_RATE);
    let mut channels: u16 = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(1);

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => {
                return Err(TranscriptionError::AudioDecode(format!(
                    "Failed to read audio packet: {error}"
                )));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                return Err(TranscriptionError::AudioDecode(
                    "Decoder reset required while reading the audio file".to_string(),
                ));
            }
            Err(error) => {
                return Err(TranscriptionError::AudioDecode(format!(
                    "Failed to decode audio packet: {error}"
                )));
            }
        };

        sample_rate = decoded.spec().rate;
        channels = decoded.spec().channels.count() as u16;
        append_interleaved_f32(decoded, &mut interleaved_samples);
    }

    if interleaved_samples.is_empty() {
        return Err(TranscriptionError::AudioDecode(
            "The selected file did not contain any decodable audio samples".to_string(),
        ));
    }

    let mono_samples = to_mono(&interleaved_samples, channels);
    let samples = if sample_rate == WHISPER_SAMPLE_RATE {
        mono_samples
    } else {
        resample(&mono_samples, sample_rate, WHISPER_SAMPLE_RATE)
    };

    let duration_ms = Some((samples.len() as u64 * 1000) / WHISPER_SAMPLE_RATE as u64);

    Ok(DecodedAudio {
        samples,
        duration_ms,
    })
}

fn decode_ogg_opus_file(path: &Path) -> Result<DecodedAudio, TranscriptionError> {
    let file = File::open(path)
        .map_err(|e| TranscriptionError::AudioDecode(format!("Cannot open audio file: {e}")))?;
    let reader = BufReader::new(file);
    let mut packets = PacketReader::new(reader);

    let header_packet = packets
        .read_packet()
        .map_err(|e| TranscriptionError::AudioDecode(format!("Failed to read OGG packet: {e}")))?
        .ok_or_else(|| {
            TranscriptionError::AudioDecode("The selected OGG file is empty".to_string())
        })?;

    let header = parse_opus_head(&header_packet.data)?;

    if header.mapping_family != 0 {
        return Err(TranscriptionError::AudioDecode(
            "OGG/Opus files with custom channel mapping are not supported yet".to_string(),
        ));
    }

    if !(1..=2).contains(&header.channel_count) {
        return Err(TranscriptionError::AudioDecode(format!(
            "OGG/Opus files with {} channels are not supported yet",
            header.channel_count
        )));
    }

    let mut decoder = OpusDecoder::new(48_000, header.channel_count as usize).map_err(|e| {
        TranscriptionError::AudioDecode(format!("Cannot initialize Opus decoder: {e}"))
    })?;

    let tags_packet = packets
        .read_packet()
        .map_err(|e| {
            TranscriptionError::AudioDecode(format!("Failed to read OGG tags packet: {e}"))
        })?
        .ok_or_else(|| {
            TranscriptionError::AudioDecode(
                "The selected OGG/Opus file is missing OpusTags".to_string(),
            )
        })?;

    if !tags_packet.data.starts_with(b"OpusTags") {
        return Err(TranscriptionError::AudioDecode(
            "The selected OGG/Opus file is missing a valid OpusTags packet".to_string(),
        ));
    }

    let channel_count = header.channel_count as usize;
    let mut pcm = Vec::<f32>::new();
    let mut skip_samples = header.pre_skip as usize;

    while let Some(packet) = packets
        .read_packet()
        .map_err(|e| TranscriptionError::AudioDecode(format!("Failed to read OGG packet: {e}")))?
    {
        if packet.data.is_empty() {
            continue;
        }

        let mut output = vec![0.0_f32; 5_760 * channel_count];
        let decoded_frames = decoder
            .decode_float(&packet.data, &mut output, false)
            .map_err(|e| {
                TranscriptionError::AudioDecode(format!("Failed to decode Opus packet: {e}"))
            })?;

        let decoded_samples = decoded_frames * channel_count;
        output.truncate(decoded_samples);

        if skip_samples > 0 {
            let skip = skip_samples.min(decoded_frames);
            let skip_values = skip * channel_count;
            output.drain(0..skip_values);
            skip_samples -= skip;
        }

        pcm.extend_from_slice(&output);
    }

    if pcm.is_empty() {
        return Err(TranscriptionError::AudioDecode(
            "The selected OGG/Opus file did not contain any decodable audio frames".to_string(),
        ));
    }

    let mono_samples = to_mono(&pcm, header.channel_count as u16);
    let samples = resample(&mono_samples, 48_000, WHISPER_SAMPLE_RATE);
    let duration_ms = Some((samples.len() as u64 * 1000) / WHISPER_SAMPLE_RATE as u64);

    Ok(DecodedAudio {
        samples,
        duration_ms,
    })
}

fn is_ogg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("ogg"))
        .unwrap_or(false)
}

struct OpusHead {
    channel_count: u8,
    pre_skip: u16,
    mapping_family: u8,
}

fn parse_opus_head(data: &[u8]) -> Result<OpusHead, TranscriptionError> {
    if data.len() < 19 || !data.starts_with(b"OpusHead") {
        return Err(TranscriptionError::AudioDecode(
            "The selected OGG file does not contain a valid OpusHead packet".to_string(),
        ));
    }

    let version = data[8];
    if version == 0 {
        return Err(TranscriptionError::AudioDecode(
            "The selected OGG/Opus file uses an invalid OpusHead version".to_string(),
        ));
    }

    let channel_count = data[9];
    let pre_skip = u16::from_le_bytes([data[10], data[11]]);
    let mapping_family = data[18];

    Ok(OpusHead {
        channel_count,
        pre_skip,
        mapping_family,
    })
}

fn append_interleaved_f32(decoded: AudioBufferRef<'_>, output: &mut Vec<f32>) {
    let capacity = decoded.capacity() as u64;
    let spec = *decoded.spec();
    let mut sample_buffer = SampleBuffer::<f32>::new(capacity, spec);
    sample_buffer.copy_interleaved_ref(decoded);
    output.extend_from_slice(sample_buffer.samples());
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut result = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let sample = if idx + 1 < samples.len() {
            samples[idx] * (1.0 - frac) + samples[idx + 1] * frac
        } else {
            samples[idx.min(samples.len() - 1)]
        };

        result.push(sample);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;

    fn write_wav_file(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        samples: &[i16],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::create(path, spec)?;
        for sample in samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        Ok(())
    }

    #[test]
    fn mono_passthrough() {
        let input = vec![0.1, 0.2, 0.3];
        let result = to_mono(&input, 1);
        assert_eq!(result, input);
    }

    #[test]
    fn stereo_to_mono_averages_channels() {
        let input = vec![0.0, 1.0, 0.5, 0.5];
        let result = to_mono(&input, 2);
        assert_eq!(result, vec![0.5, 0.5]);
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = vec![0.1, 0.2, 0.3];
        let result = resample(&input, 16000, 16000);
        assert_eq!(result, input);
    }

    #[test]
    fn resample_empty_returns_empty() {
        let result = resample(&[], 44100, 16000);
        assert!(result.is_empty());
    }

    #[test]
    fn resample_downsamples() {
        let input: Vec<f32> = (0..44100).map(|i| i as f32 / 44100.0).collect();
        let result = resample(&input, 44100, 16000);
        let expected_len = (44100.0_f64 / (44100.0_f64 / 16000.0_f64)).ceil() as usize;
        assert!((result.len() as i64 - expected_len as i64).abs() <= 1);
    }

    #[test]
    fn decode_audio_file_reads_wav_into_mono_whisper_pcm() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("sample.wav");
        let mut samples = Vec::new();
        for _ in 0..160 {
            samples.push(16_384_i16);
            samples.push(8_192_i16);
        }

        write_wav_file(&path, 16_000, 2, &samples).expect("write wav");

        let decoded = decode_audio_file(&path).expect("decode wav");

        assert_eq!(decoded.duration_ms, Some(10));
        assert_eq!(decoded.samples.len(), 160);
        let expected = (16_384_f32 / i16::MAX as f32 + 8_192_f32 / i16::MAX as f32) / 2.0;
        assert!((decoded.samples[0] - expected).abs() < 0.02);
        assert!(decoded
            .samples
            .iter()
            .all(|sample| (*sample - expected).abs() < 0.02));
    }

    #[test]
    fn decode_audio_file_returns_audio_decode_error_for_invalid_input() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("invalid.wav");
        fs::write(&path, b"not actually a wav file").expect("write invalid file");

        match decode_audio_file(&path) {
            Err(TranscriptionError::AudioDecode(message)) => {
                assert!(!message.is_empty());
                assert!(message.contains("Unsupported audio file"));
            }
            Ok(_) => panic!("invalid file should fail"),
            Err(other) => panic!("expected audio decode error, got {other:?}"),
        }
    }

    #[test]
    fn encode_mp3_for_upload_produces_smaller_mp3_from_wav() {
        let temp_dir = TempDir::new().expect("temp dir");
        let wav_path = temp_dir.path().join("input.wav");
        let mp3_path = temp_dir.path().join("output.mp3");

        // 1 second of a 440 Hz sine wave, 16 kHz mono.
        let samples: Vec<i16> = (0..16_000)
            .map(|i| {
                (f32::sin(2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0) * 16_000.0)
                    as i16
            })
            .collect();
        write_wav_file(&wav_path, 16_000, 1, &samples).expect("write wav");

        encode_mp3_for_upload(&wav_path, &mp3_path).expect("encode mp3");

        let mp3_size = fs::metadata(&mp3_path).expect("mp3 metadata").len();
        let wav_size = fs::metadata(&wav_path).expect("wav metadata").len();
        assert!(mp3_size > 0, "MP3 file should be non-empty");
        assert!(
            mp3_size < wav_size,
            "64 kbps MP3 ({mp3_size} B) should be smaller than WAV ({wav_size} B)"
        );
    }

    #[test]
    fn encode_mp3_for_upload_returns_error_for_invalid_audio() {
        let temp_dir = TempDir::new().expect("temp dir");
        let bad_path = temp_dir.path().join("garbage.wav");
        let mp3_path = temp_dir.path().join("output.mp3");

        fs::write(&bad_path, b"not audio data").expect("write garbage file");

        match encode_mp3_for_upload(&bad_path, &mp3_path) {
            Err(TranscriptionError::AudioDecode(_)) => {}
            Err(other) => panic!("expected AudioDecode error, got {other:?}"),
            Ok(()) => panic!("encoding invalid audio should fail"),
        }

        assert!(
            !mp3_path.exists(),
            "MP3 output should not be created when encoding fails"
        );
    }
}
