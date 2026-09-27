use std::fs::File;
use std::path::Path;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioAnalysis {
    pub bpm: Option<f64>,
    pub pitch_key: Option<String>,
}

/// Caps how much audio gets decoded/analyzed per file so scanning a large
/// library stays bounded regardless of individual track length — plenty for
/// a stable tempo/key read on typical samples, loops, and even full songs.
const MAX_ANALYSIS_SECONDS: f64 = 30.0;

const BPM_MIN: f64 = 60.0;
const BPM_MAX: f64 = 200.0;

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

// Krumhansl-Kessler key profiles: relative perceptual weight of each pitch
// class (0 = tonic) within a major/minor key, used as correlation templates.
const MAJOR_PROFILE: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Decodes up to `MAX_ANALYSIS_SECONDS` of a file and estimates tempo/key
/// from it. Returns None only if the file can't be decoded at all; the two
/// fields inside can still independently be None if that specific estimate
/// isn't confident (too short, silent, or no clear periodicity/tonality).
pub fn analyze(path: &Path) -> Option<AudioAnalysis> {
    let (samples, sample_rate) = decode_mono(path)?;
    if samples.is_empty() {
        return None;
    }

    Some(AudioAnalysis {
        bpm: detect_bpm(&samples, sample_rate),
        pitch_key: detect_key(&samples, sample_rate),
    })
}

fn decode_mono(path: &Path) -> Option<(Vec<f32>, u32)> {
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
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())?
        .clone();
    let sample_rate = track.codec_params.sample_rate?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let max_samples = (sample_rate as f64 * MAX_ANALYSIS_SECONDS) as usize;
    let mut mono: Vec<f32> = Vec::with_capacity(max_samples.min(1 << 20));
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    while mono.len() < max_samples {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if sample_buf.is_none() {
            sample_buf = Some(SampleBuffer::<f32>::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }

        let channels = decoded.spec().channels.count().max(1);
        if let Some(buf) = sample_buf.as_mut() {
            buf.copy_interleaved_ref(decoded);
            for frame in buf.samples().chunks(channels) {
                mono.push(frame.iter().sum::<f32>() / channels as f32);
            }
        }
    }

    mono.truncate(max_samples);
    Some((mono, sample_rate))
}

/// Estimates tempo by autocorrelating a short-time-energy onset envelope
/// over the lag range corresponding to BPM_MIN..BPM_MAX. Simple but
/// effective for the percussive, rhythmically-steady material (loops,
/// one-shots, beats) this app is built around.
pub(crate) fn detect_bpm(samples: &[f32], sample_rate: u32) -> Option<f64> {
    const FRAME_SIZE: usize = 512;
    let frame_rate = sample_rate as f64 / FRAME_SIZE as f64;

    let envelope: Vec<f32> = samples
        .chunks(FRAME_SIZE)
        .map(|frame| (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt())
        .collect();

    let onset: Vec<f32> = envelope
        .windows(2)
        .map(|w| (w[1] - w[0]).max(0.0))
        .collect();

    let min_lag = (60.0 / BPM_MAX * frame_rate).round().max(1.0) as usize;
    let max_lag = (60.0 / BPM_MIN * frame_rate).round() as usize;

    // Need enough onset frames for the longest lag under test, with
    // headroom so the correlation isn't dominated by a handful of pairs.
    if min_lag >= max_lag || onset.len() < max_lag * 2 {
        return None;
    }

    let mut scores: Vec<(usize, f64)> = Vec::with_capacity(max_lag - min_lag + 1);
    let mut best_score = 0.0f64;
    for lag in min_lag..=max_lag {
        let score: f64 = onset
            .iter()
            .zip(onset.iter().skip(lag))
            .map(|(a, b)| (*a as f64) * (*b as f64))
            .sum();
        best_score = best_score.max(score);
        scores.push((lag, score));
    }

    if best_score <= 0.0 {
        return None;
    }

    // A periodic onset pattern also autocorrelates strongly at integer
    // multiples of its true period (2x, 3x, ...), so the raw global-max lag
    // can lock onto a half- or third-tempo "octave" reading instead of the
    // fundamental. Prefer the smallest lag whose score is close to the
    // global max — the true period is always the shortest such peak.
    const OCTAVE_THRESHOLD: f64 = 0.85;
    let best_lag = scores
        .iter()
        .find(|(_, score)| *score >= best_score * OCTAVE_THRESHOLD)
        .map(|(lag, _)| *lag)?;

    let bpm = 60.0 / (best_lag as f64 / frame_rate);
    Some((bpm * 100.0).round() / 100.0)
}

/// Estimates the musical key by building a chroma (12 pitch-class) profile
/// from short-time FFTs across the signal, then correlating it against the
/// standard Krumhansl-Kessler major/minor key templates.
pub(crate) fn detect_key(samples: &[f32], sample_rate: u32) -> Option<String> {
    const WINDOW_SIZE: usize = 4096;
    const HOP_SIZE: usize = 2048;
    const MIN_FREQ: f64 = 27.5; // A0
    const MAX_FREQ: f64 = 5000.0;

    if samples.len() < WINDOW_SIZE {
        return None;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW_SIZE);

    let mut chroma = [0.0f64; 12];
    let mut window_start = 0;

    while window_start + WINDOW_SIZE <= samples.len() {
        let mut buffer: Vec<Complex32> = samples[window_start..window_start + WINDOW_SIZE]
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let hann = 0.5
                    - 0.5
                        * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW_SIZE as f32 - 1.0))
                            .cos();
                Complex32::new(s * hann, 0.0)
            })
            .collect();

        fft.process(&mut buffer);

        for (bin, value) in buffer.iter().take(WINDOW_SIZE / 2).enumerate().skip(1) {
            let freq = bin as f64 * sample_rate as f64 / WINDOW_SIZE as f64;
            if freq < MIN_FREQ || freq > MAX_FREQ {
                continue;
            }
            let magnitude = (value.re as f64).hypot(value.im as f64);
            chroma[freq_to_pitch_class(freq)] += magnitude;
        }

        window_start += HOP_SIZE;
    }

    if chroma.iter().all(|&v| v == 0.0) {
        return None;
    }

    best_key_match(&chroma)
}

fn freq_to_pitch_class(freq: f64) -> usize {
    let note = 69.0 + 12.0 * (freq / 440.0).log2(); // MIDI note number, A4 = 69
    (note.round() as i64).rem_euclid(12) as usize
}

fn best_key_match(chroma: &[f64; 12]) -> Option<String> {
    let mut best_name: Option<String> = None;
    let mut best_score = f64::MIN;

    for root in 0..12 {
        let major_score = correlate(chroma, &MAJOR_PROFILE, root);
        if major_score > best_score {
            best_score = major_score;
            best_name = Some(format!("{} maj", NOTE_NAMES[root]));
        }
        let minor_score = correlate(chroma, &MINOR_PROFILE, root);
        if minor_score > best_score {
            best_score = minor_score;
            best_name = Some(format!("{} min", NOTE_NAMES[root]));
        }
    }

    best_name
}

/// Pearson correlation between the observed chroma vector and `profile`
/// rotated so its tonic (index 0) sits at pitch class `root`.
fn correlate(chroma: &[f64; 12], profile: &[f64; 12], root: usize) -> f64 {
    let rotated: [f64; 12] = std::array::from_fn(|i| profile[(i + 12 - root) % 12]);

    let chroma_mean = chroma.iter().sum::<f64>() / 12.0;
    let profile_mean = rotated.iter().sum::<f64>() / 12.0;

    let mut numerator = 0.0;
    let mut chroma_var = 0.0;
    let mut profile_var = 0.0;
    for i in 0..12 {
        let c = chroma[i] - chroma_mean;
        let p = rotated[i] - profile_mean;
        numerator += c * p;
        chroma_var += c * c;
        profile_var += p * p;
    }

    if chroma_var <= 0.0 || profile_var <= 0.0 {
        return f64::MIN;
    }

    numerator / (chroma_var.sqrt() * profile_var.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn synthetic_click_train(sample_rate: u32, bpm: f64, duration_secs: f64) -> Vec<f32> {
        let total = (sample_rate as f64 * duration_secs) as usize;
        let beat_interval = (sample_rate as f64 * 60.0 / bpm) as usize;
        let click_len = (sample_rate as f64 * 0.03) as usize;

        let mut samples = vec![0.0f32; total];
        let mut start = 0;
        while start < total {
            for i in 0..click_len.min(total - start) {
                let t = i as f64 / sample_rate as f64;
                let envelope = 1.0 - (i as f64 / click_len as f64);
                samples[start + i] =
                    (0.8 * envelope * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as f32;
            }
            start += beat_interval;
        }
        samples
    }

    fn synthetic_tone(sample_rate: u32, freq: f64, duration_secs: f64) -> Vec<f32> {
        let n = (sample_rate as f64 * duration_secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (0.5 * (2.0 * std::f64::consts::PI * freq * t).sin()) as f32
            })
            .collect()
    }

    #[test]
    fn detects_bpm_from_click_train_within_tolerance() {
        let samples = synthetic_click_train(44100, 128.0, 8.0);
        let bpm = detect_bpm(&samples, 44100).expect("expected a bpm estimate");
        assert!((bpm - 128.0).abs() < 3.0, "expected ~128 BPM, got {bpm}");
    }

    #[test]
    fn detects_different_tempo_correctly() {
        let samples = synthetic_click_train(44100, 90.0, 8.0);
        let bpm = detect_bpm(&samples, 44100).expect("expected a bpm estimate");
        assert!((bpm - 90.0).abs() < 3.0, "expected ~90 BPM, got {bpm}");
    }

    #[test]
    fn bpm_is_none_for_silence() {
        let samples = vec![0.0f32; 44100 * 4];
        assert!(detect_bpm(&samples, 44100).is_none());
    }

    #[test]
    fn bpm_is_none_for_too_short_audio() {
        let samples = vec![0.1f32; 4096];
        assert!(detect_bpm(&samples, 44100).is_none());
    }

    #[test]
    fn detects_root_note_from_pure_tone() {
        // A4 = 440Hz. Major vs minor is inherently ambiguous for a single
        // sine tone with no harmonic content, so only assert the root.
        let samples = synthetic_tone(44100, 440.0, 2.0);
        let key = detect_key(&samples, 44100).expect("expected a key estimate");
        assert!(key.starts_with("A "), "expected root note A, got {key}");
    }

    #[test]
    fn detects_different_root_note() {
        // C4 = 261.63Hz.
        let samples = synthetic_tone(44100, 261.63, 2.0);
        let key = detect_key(&samples, 44100).expect("expected a key estimate");
        assert!(key.starts_with("C "), "expected root note C, got {key}");
    }

    #[test]
    fn key_is_none_for_silence() {
        let samples = vec![0.0f32; 44100 * 2];
        assert!(detect_key(&samples, 44100).is_none());
    }

    fn write_wav_i16(path: &Path, sample_rate: u32, samples: &[f32]) {
        let bits_per_sample: u16 = 16;
        let channels: u16 = 1;
        let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = channels * (bits_per_sample / 8);
        let data_size = (samples.len() * 2) as u32;

        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_size).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&block_align.to_le_bytes()).unwrap();
        f.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();
        for s in samples {
            let clamped = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            f.write_all(&clamped.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn analyze_end_to_end_on_real_wav_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("cadence_analysis_e2e_{}.wav", std::process::id()));
        let samples = synthetic_click_train(44100, 120.0, 6.0);
        write_wav_i16(&path, 44100, &samples);

        let result = analyze(&path).expect("expected analysis to succeed");
        let bpm = result.bpm.expect("expected a bpm estimate");
        assert!((bpm - 120.0).abs() < 5.0, "expected ~120 BPM, got {bpm}");

        std::fs::remove_file(&path).ok();
    }
}
