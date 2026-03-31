use std::path::Path;

use crate::providers::TranscriptionError;

const WHISPER_SAMPLE_RATE: u32 = 16_000;

pub fn decode_wav_file(path: &Path) -> Result<Vec<f32>, TranscriptionError> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| TranscriptionError::AudioDecode(format!("Cannot open WAV file: {e}")))?;

    let spec = reader.spec();

    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TranscriptionError::AudioDecode(format!("Error reading samples: {e}")))?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    TranscriptionError::AudioDecode(format!("Error reading samples: {e}"))
                })?
                .into_iter()
                .map(|s| s as f32 / max)
                .collect()
        }
    };

    let mono = to_mono(&raw_samples, spec.channels);

    if spec.sample_rate == WHISPER_SAMPLE_RATE {
        Ok(mono)
    } else {
        Ok(resample(&mono, spec.sample_rate, WHISPER_SAMPLE_RATE))
    }
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
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
}
