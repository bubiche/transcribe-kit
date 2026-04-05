use std::path::Path;

use super::LiveRecordingError;

pub(super) fn mix_wav_files(
    mic_path: &Path,
    loopback_path: &Path,
    output_path: &Path,
) -> Result<(), LiveRecordingError> {
    let (mic_samples, mic_sample_rate_hz) = read_wav_as_mono_i16(mic_path)?;
    let (loopback_samples, loopback_sample_rate_hz) = read_wav_as_mono_i16(loopback_path)?;

    let loopback_samples = if mic_sample_rate_hz == loopback_sample_rate_hz {
        loopback_samples
    } else {
        eprintln!(
            "Mixing WAV files with different sample rates: mic={} Hz loopback={} Hz. Resampling loopback.",
            mic_sample_rate_hz, loopback_sample_rate_hz
        );
        resample_linear_i16(
            &loopback_samples,
            loopback_sample_rate_hz,
            mic_sample_rate_hz,
        )
    };

    let mut writer = hound::WavWriter::create(output_path, wav_spec(mic_sample_rate_hz, 1))
        .map_err(|error| LiveRecordingError::CreateWav(error.to_string()))?;

    let max_len = mic_samples.len().max(loopback_samples.len());
    for index in 0..max_len {
        let sample = match (
            mic_samples.get(index).copied(),
            loopback_samples.get(index).copied(),
        ) {
            (Some(mic), Some(loopback)) => mix_sample(mic, loopback),
            (Some(mic), None) => mic,
            (None, Some(loopback)) => loopback,
            (None, None) => break,
        };
        writer
            .write_sample(sample)
            .map_err(|error| LiveRecordingError::FinalizeWav(error.to_string()))?;
    }

    writer
        .finalize()
        .map_err(|error| LiveRecordingError::FinalizeWav(error.to_string()))?;
    Ok(())
}

fn mix_sample(a: i16, b: i16) -> i16 {
    (a as i32 + b as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn read_wav_as_mono_i16(path: &Path) -> Result<(Vec<i16>, u32), LiveRecordingError> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| LiveRecordingError::ReadWav(error.to_string()))?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return Err(LiveRecordingError::ReadWav(format!(
            "unsupported WAV format (sample_format={:?}, bits_per_sample={})",
            spec.sample_format, spec.bits_per_sample
        )));
    }

    let interleaved = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| LiveRecordingError::ReadWav(error.to_string()))?;
    Ok((
        downmix_to_mono(interleaved, spec.channels),
        spec.sample_rate,
    ))
}

fn downmix_to_mono(interleaved_samples: Vec<i16>, channels: u16) -> Vec<i16> {
    if channels <= 1 {
        return interleaved_samples;
    }

    let channels = channels as usize;
    interleaved_samples
        .chunks(channels)
        .map(|frame| {
            let sum = frame.iter().map(|sample| *sample as i32).sum::<i32>();
            (sum / frame.len() as i32) as i16
        })
        .collect()
}

fn resample_linear_i16(samples: &[i16], source_rate_hz: u32, target_rate_hz: u32) -> Vec<i16> {
    if source_rate_hz == target_rate_hz {
        return samples.to_vec();
    }
    if source_rate_hz == 0 || target_rate_hz == 0 || samples.is_empty() {
        return Vec::new();
    }
    if samples.len() == 1 {
        return samples.to_vec();
    }

    let ratio = target_rate_hz as f64 / source_rate_hz as f64;
    let target_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let source_step = source_rate_hz as f64 / target_rate_hz as f64;

    (0..target_len)
        .map(|target_index| {
            let source_pos = target_index as f64 * source_step;
            let source_index = source_pos.floor() as usize;
            let frac = source_pos - source_index as f64;

            if source_index + 1 >= samples.len() {
                return samples[samples.len() - 1];
            }

            let a = samples[source_index] as f64;
            let b = samples[source_index + 1] as f64;
            (a + ((b - a) * frac))
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect()
}

pub(super) fn duration_ms_from_wav_file(path: &Path) -> Result<u64, LiveRecordingError> {
    let reader = hound::WavReader::open(path)
        .map_err(|error| LiveRecordingError::ReadWav(error.to_string()))?;
    Ok(duration_ms_from_frames(
        reader.duration() as u64,
        reader.spec().sample_rate,
    ))
}

pub(super) fn wav_is_silent(path: &Path) -> bool {
    let Ok(mut reader) = hound::WavReader::open(path) else {
        return true;
    };
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return false;
    }
    reader.samples::<i16>().all(|s| s.map(|v| v == 0).unwrap_or(true))
}

pub(super) fn wav_spec(sample_rate_hz: u32, channels: u16) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

pub(super) fn duration_ms_from_frames(frame_count: u64, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }

    (frame_count.saturating_mul(1000)) / sample_rate_hz as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn mix_wav_files_combines_equal_length_mono_files() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mic_path = temp_dir.path().join("mic.wav");
        let loopback_path = temp_dir.path().join("loopback.wav");
        let output_path = temp_dir.path().join("mixed.wav");

        write_test_wav(&mic_path, 16_000, 1, &[100, -100, 25]);
        write_test_wav(&loopback_path, 16_000, 1, &[10, 20, -30]);

        mix_wav_files(&mic_path, &loopback_path, &output_path).expect("mix");

        assert_eq!(read_test_samples(&output_path), vec![110, -80, -5]);
    }

    #[test]
    fn mix_wav_files_writes_remaining_samples_from_longer_file() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mic_path = temp_dir.path().join("mic.wav");
        let loopback_path = temp_dir.path().join("loopback.wav");
        let output_path = temp_dir.path().join("mixed.wav");

        write_test_wav(&mic_path, 16_000, 1, &[100, 100]);
        write_test_wav(&loopback_path, 16_000, 1, &[50, 60, 70, 80]);

        mix_wav_files(&mic_path, &loopback_path, &output_path).expect("mix");

        assert_eq!(read_test_samples(&output_path), vec![150, 160, 70, 80]);
    }

    #[test]
    fn mix_wav_files_downmixes_stereo_to_mono_before_mixing() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mic_path = temp_dir.path().join("mic_stereo.wav");
        let loopback_path = temp_dir.path().join("loopback_mono.wav");
        let output_path = temp_dir.path().join("mixed.wav");

        write_test_wav(&mic_path, 16_000, 2, &[100, 300, 200, 400]);
        write_test_wav(&loopback_path, 16_000, 1, &[10, 20]);

        mix_wav_files(&mic_path, &loopback_path, &output_path).expect("mix");

        assert_eq!(read_test_samples(&output_path), vec![210, 320]);
    }

    #[test]
    fn mix_wav_files_clamps_summed_samples_to_i16_range() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mic_path = temp_dir.path().join("mic.wav");
        let loopback_path = temp_dir.path().join("loopback.wav");
        let output_path = temp_dir.path().join("mixed.wav");

        write_test_wav(&mic_path, 16_000, 1, &[i16::MAX, i16::MIN]);
        write_test_wav(&loopback_path, 16_000, 1, &[100, -100]);

        mix_wav_files(&mic_path, &loopback_path, &output_path).expect("mix");

        assert_eq!(read_test_samples(&output_path), vec![i16::MAX, i16::MIN]);
    }

    #[test]
    fn mix_wav_files_uses_mic_sample_rate_for_output() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mic_path = temp_dir.path().join("mic.wav");
        let loopback_path = temp_dir.path().join("loopback.wav");
        let output_path = temp_dir.path().join("mixed.wav");

        write_test_wav(&mic_path, 16_000, 1, &[1000, 0, -1000, 500]);
        write_test_wav(&loopback_path, 8_000, 1, &[500, -500]);

        mix_wav_files(&mic_path, &loopback_path, &output_path).expect("mix");

        let reader = hound::WavReader::open(&output_path).expect("open output");
        assert_eq!(reader.spec().sample_rate, 16_000);
    }

    #[test]
    fn duration_ms_uses_frame_count() {
        assert_eq!(duration_ms_from_frames(16_000, 16_000), 1000);
        assert_eq!(duration_ms_from_frames(8_000, 16_000), 500);
    }

    fn write_test_wav(path: &Path, sample_rate_hz: u32, channels: u16, samples: &[i16]) {
        let mut writer =
            hound::WavWriter::create(path, wav_spec(sample_rate_hz, channels)).expect("create wav");
        for sample in samples {
            writer.write_sample(*sample).expect("write sample");
        }
        writer.finalize().expect("finalize wav");
    }

    fn read_test_samples(path: &Path) -> Vec<i16> {
        let mut reader = hound::WavReader::open(path).expect("open wav");
        reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .expect("samples")
    }
}
