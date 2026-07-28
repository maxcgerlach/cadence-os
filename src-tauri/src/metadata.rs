use std::fs::File;
use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioMetadata {
    pub duration_ms: i64,
    pub sample_rate: i64,
    pub channels: i64,
}

/// Probes an audio file's header (does not decode the full stream) to pull
/// duration/sample rate/channel count. Returns None for files symphonia
/// can't parse rather than failing the whole scan.
pub fn extract(path: &Path) -> Option<AudioMetadata> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())?;

    let params = &track.codec_params;
    let sample_rate = params.sample_rate?;
    let channels = params.channels.map(|c| c.count()).unwrap_or(0);
    let n_frames = params.n_frames?;
    let duration_ms = (n_frames as f64 / sample_rate as f64 * 1000.0).round() as i64;

    Some(AudioMetadata {
        duration_ms,
        sample_rate: sample_rate as i64,
        channels: channels as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a minimal valid PCM WAV file: `num_frames` mono samples at
    /// `sample_rate` Hz, all zero. Enough for symphonia's WAV reader to
    /// parse header + frame count without any external fixture files.
    fn write_test_wav(path: &Path, sample_rate: u32, num_frames: u32) {
        let bits_per_sample: u16 = 16;
        let channels: u16 = 1;
        let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = channels * (bits_per_sample / 8);
        let data_size = num_frames * block_align as u32;

        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();

        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap(); // fmt chunk size
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&block_align.to_le_bytes()).unwrap();
        f.write_all(&bits_per_sample.to_le_bytes()).unwrap();

        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();
        f.write_all(&vec![0u8; data_size as usize]).unwrap();
    }

    #[test]
    fn extracts_duration_sample_rate_and_channels_from_wav() {
        let dir = std::env::temp_dir();
        let path = dir.join("cadence_metadata_test.wav");
        write_test_wav(&path, 44100, 44100); // exactly 1 second

        let meta = extract(&path).expect("expected metadata to be extracted");

        assert_eq!(meta.sample_rate, 44100);
        assert_eq!(meta.channels, 1);
        assert_eq!(meta.duration_ms, 1000);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_none_for_non_audio_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("cadence_metadata_test_not_audio.wav");
        std::fs::write(&path, b"not a real wav file").unwrap();

        assert!(extract(&path).is_none());

        std::fs::remove_file(&path).ok();
    }
}
