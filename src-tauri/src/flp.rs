use std::fs;
use std::path::Path;

// FLP event-ID size classes: an event's ID byte determines how its value is
// encoded. IDs below WORD are a single byte, below DWORD are 2 bytes,
// below TEXT are 4 bytes, and TEXT and above are a VarInt-prefixed
// variable-length block. Verified against the actual pyflp parser source
// (github.com/demberto/PyFLP), not reverse-engineered from scratch here.
const WORD: u16 = 64;
const DWORD: u16 = 128;
const TEXT: u16 = 192;

// Legacy tempo, pre FL Studio 3.4.0: whole BPM only, stored as a WORD.
const EVENT_TEMPO_COARSE: u8 = 66; // WORD + 2
// Legacy tempo fractional part (added in FL Studio 3.4.0), value/1000.
const EVENT_TEMPO_FINE: u8 = 93; // WORD + 29
// Modern tempo (single event): a DWORD, actual BPM = value / 1000.
const EVENT_TEMPO: u8 = 156; // DWORD + 28
// Absolute path of the sample file loaded into a Sampler/Instrument
// channel. One project can (and typically does) have many of these — one
// per channel. Contains "%FLStudioFactoryData%..." for stock samples.
const EVENT_SAMPLE_PATH: u8 = 196; // TEXT + 4
// The FL Studio version string that saved the file, e.g. "25.2.5.5055".
// Always ASCII regardless of what the rest of the file's TEXT events use
// (pyflp source: kept ASCII for backward compat with pre-11.5 readers).
// Its own major.minor determines whether every OTHER text event in this
// file is ASCII (pre-11.5) or UTF-16LE (11.5+).
const EVENT_FL_VERSION: u8 = 199; // TEXT + 7

pub struct ProjectData {
    pub tempo: Option<f64>,
    /// Absolute paths of every sample referenced by a channel in this
    /// project, in file order. Can contain duplicates (the same sample
    /// used in multiple channels) and stock-library placeholder paths
    /// (containing "%FLStudioFactoryData%").
    pub sample_paths: Vec<String>,
}

/// Reads project tempo and referenced sample paths directly out of an FL
/// Studio `.flp` file's binary event stream — exact, not estimated.
///
/// Known limitation, verified against real files (not hypothetical): FL
/// Studio 25 projects containing certain newer VST plugin events can
/// desync the event walk partway through the file and yield an incomplete
/// result even though the file has more data. This isn't unique to this
/// implementation — the reference `pyflp` parser (the most complete public
/// FLP parser) hits the exact same "Unknown VST marker" issue on the same
/// files as of 2026. Returning whatever was found before the desync is the
/// correct, honest behavior rather than guessing further; FL Studio 21
/// projects (and presumably other pre-25 versions) parse correctly and
/// completely.
pub fn parse(path: &Path) -> Option<ProjectData> {
    let data = fs::read(path).ok()?;
    if data.len() < 22 || &data[0..4] != b"FLhd" || &data[14..18] != b"FLdt" {
        return None;
    }

    let events_size = u32::from_le_bytes(data[18..22].try_into().ok()?) as usize;
    let end = (22 + events_size).min(data.len());

    let mut pos = 22;
    let mut tempo: Option<u32> = None;
    let mut tempo_coarse: Option<u16> = None;
    let mut tempo_fine: Option<u16> = None;
    let mut sample_paths: Vec<String> = Vec::new();
    // None = not yet determined (no text event decoded until we know this;
    // matches pyflp's own requirement that FLVersion appears before any
    // other text event in a well-formed file).
    let mut unicode_text: Option<bool> = None;

    while pos < end {
        let id = data[pos];
        pos += 1;
        let id_class = id as u16;

        if id_class < WORD {
            if pos + 1 > end {
                break;
            }
            pos += 1;
        } else if id_class < DWORD {
            if pos + 2 > end {
                break;
            }
            let value = u16::from_le_bytes([data[pos], data[pos + 1]]);
            match id {
                EVENT_TEMPO_COARSE => tempo_coarse = Some(value),
                EVENT_TEMPO_FINE => tempo_fine = Some(value),
                _ => {}
            }
            pos += 2;
        } else if id_class < TEXT {
            if pos + 4 > end {
                break;
            }
            // Slicing exactly 4 bytes into a [u8; 4] cannot fail here; this
            // is not a "give up on the whole file" condition, unlike the
            // varint case below.
            let value = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            if id == EVENT_TEMPO {
                tempo = Some(value);
            }
            pos += 4;
        } else {
            // A malformed/truncated event anywhere later in a large,
            // complex file must not discard results already found earlier
            // in the stream — stop walking, but keep what we have.
            let Some((size, varint_len)) = read_varint(&data[pos..end]) else {
                break;
            };
            pos += varint_len;
            if pos + size > end {
                break;
            }
            let payload = &data[pos..pos + size];

            if id == EVENT_FL_VERSION {
                if let Ok(s) = std::str::from_utf8(payload) {
                    if let Some((major, minor)) = parse_major_minor(s.trim_end_matches('\0')) {
                        unicode_text = Some((major, minor) >= (11, 5));
                    }
                }
            } else if id == EVENT_SAMPLE_PATH {
                if let Some(unicode) = unicode_text {
                    if let Some(s) = decode_text(payload, unicode) {
                        if !s.is_empty() {
                            sample_paths.push(s);
                        }
                    }
                }
            }

            pos += size;
        }
    }

    let resolved_tempo = tempo.map(|v| v as f64 / 1000.0).or_else(|| {
        tempo_coarse.map(|coarse| coarse as f64 + tempo_fine.unwrap_or(0) as f64 / 1000.0)
    });

    Some(ProjectData {
        tempo: resolved_tempo,
        sample_paths,
    })
}

fn decode_text(bytes: &[u8], unicode: bool) -> Option<String> {
    if unicode {
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
    } else {
        std::str::from_utf8(bytes)
            .ok()
            .map(|s| s.trim_end_matches('\0').to_string())
    }
}

fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Unsigned LEB128 (7 bits per byte, high bit = more bytes follow) — the
/// length prefix `construct.VarInt` uses ahead of every FLP TEXT/DATA event.
/// Returns (value, bytes_consumed).
fn read_varint(buf: &[u8]) -> Option<(usize, usize)> {
    let mut value: usize = 0;
    let mut shift = 0u32;
    for (i, &byte) in buf.iter().enumerate() {
        value |= ((byte & 0x7F) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_varint(buf: &mut Vec<u8>, mut value: usize) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    /// Builds a minimal but structurally valid FLP byte stream: real
    /// header/chunk framing, plus whatever `events` bytes the caller wants
    /// in the data chunk (already-encoded event bytes, not decoded values).
    fn build_flp(events: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"FLhd");
        buf.extend_from_slice(&6u32.to_le_bytes());
        buf.extend_from_slice(&0i16.to_le_bytes()); // format
        buf.extend_from_slice(&1u16.to_le_bytes()); // channel_count
        buf.extend_from_slice(&96u16.to_le_bytes()); // ppq (must be a VALID_PPQS value upstream, doesn't matter here)
        buf.extend_from_slice(b"FLdt");
        buf.extend_from_slice(&(events.len() as u32).to_le_bytes());
        buf.extend_from_slice(events);
        buf
    }

    fn dword_event(id: u8, value: u32) -> Vec<u8> {
        let mut e = vec![id];
        e.extend_from_slice(&value.to_le_bytes());
        e
    }

    fn word_event(id: u8, value: u16) -> Vec<u8> {
        let mut e = vec![id];
        e.extend_from_slice(&value.to_le_bytes());
        e
    }

    fn text_event_ascii(id: u8, text: &str) -> Vec<u8> {
        let mut e = vec![id];
        let bytes = format!("{text}\0").into_bytes();
        write_varint(&mut e, bytes.len());
        e.extend_from_slice(&bytes);
        e
    }

    fn text_event_utf16(id: u8, text: &str) -> Vec<u8> {
        let mut e = vec![id];
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes()); // null terminator
        write_varint(&mut e, bytes.len());
        e.extend_from_slice(&bytes);
        e
    }

    /// FLVersion is always the first text event and is always ASCII,
    /// regardless of which encoding the rest of the file's text uses.
    fn fl_version_event(version: &str) -> Vec<u8> {
        text_event_ascii(EVENT_FL_VERSION, version)
    }

    fn write_flp_file(path: &Path, bytes: &[u8]) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    fn temp_flp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cadence_flp_{name}_{}.flp", std::process::id()))
    }

    #[test]
    fn reads_modern_tempo_event() {
        let mut events = Vec::new();
        events.extend(fl_version_event("21.2.1.3"));
        events.extend(dword_event(EVENT_TEMPO, 132_510)); // 132.51 BPM
        events.extend(dword_event(159, 20)); // FLBuild = DWORD+31, unrelated, after

        let path = temp_flp_path("modern_tempo");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().tempo, Some(132.51));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn falls_back_to_legacy_coarse_and_fine_tempo_events() {
        let mut events = Vec::new();
        events.extend(fl_version_event("3.4.0"));
        events.extend(word_event(EVENT_TEMPO_COARSE, 140));
        events.extend(word_event(EVENT_TEMPO_FINE, 500));

        let path = temp_flp_path("legacy_tempo");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().tempo, Some(140.5));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn legacy_coarse_only_with_no_fine_event() {
        let mut events = fl_version_event("3.3.0");
        events.extend(word_event(EVENT_TEMPO_COARSE, 128));

        let path = temp_flp_path("coarse_only");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().tempo, Some(128.0));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn modern_event_takes_precedence_over_legacy_ones() {
        let mut events = Vec::new();
        events.extend(fl_version_event("21.0.0"));
        events.extend(word_event(EVENT_TEMPO_COARSE, 90));
        events.extend(dword_event(EVENT_TEMPO, 174_000));

        let path = temp_flp_path("precedence");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().tempo, Some(174.0));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_no_tempo_when_absent() {
        let events = fl_version_event("21.0.0");

        let path = temp_flp_path("no_tempo");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().tempo, None);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_none_for_garbage_file() {
        let path = temp_flp_path("garbage");
        write_flp_file(&path, b"not an flp file at all");

        assert!(parse(&path).is_none());

        fs::remove_file(&path).ok();
    }

    #[test]
    fn collects_sample_paths_from_modern_unicode_project() {
        let mut events = Vec::new();
        events.extend(fl_version_event("21.2.1.3")); // >= 11.5 -> UTF-16
        events.extend(text_event_utf16(EVENT_SAMPLE_PATH, "/Users/max/samples/kick.wav"));
        events.extend(text_event_utf16(EVENT_SAMPLE_PATH, "/Users/max/samples/snare.wav"));

        let path = temp_flp_path("unicode_samples");
        write_flp_file(&path, &build_flp(&events));

        let project = parse(&path).unwrap();
        assert_eq!(
            project.sample_paths,
            vec!["/Users/max/samples/kick.wav", "/Users/max/samples/snare.wav"]
        );

        fs::remove_file(&path).ok();
    }

    #[test]
    fn collects_sample_paths_from_legacy_ascii_project() {
        let mut events = Vec::new();
        events.extend(fl_version_event("9.0.0")); // < 11.5 -> ASCII
        events.extend(text_event_ascii(EVENT_SAMPLE_PATH, "C:\\samples\\kick.wav"));

        let path = temp_flp_path("ascii_samples");
        write_flp_file(&path, &build_flp(&events));

        let project = parse(&path).unwrap();
        assert_eq!(project.sample_paths, vec!["C:\\samples\\kick.wav"]);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn duplicate_sample_references_are_all_preserved() {
        let mut events = Vec::new();
        events.extend(fl_version_event("21.2.1.3"));
        events.extend(text_event_utf16(EVENT_SAMPLE_PATH, "/lib/kick.wav"));
        events.extend(text_event_utf16(EVENT_SAMPLE_PATH, "/lib/kick.wav"));

        let path = temp_flp_path("dup_refs");
        write_flp_file(&path, &build_flp(&events));

        let project = parse(&path).unwrap();
        assert_eq!(project.sample_paths, vec!["/lib/kick.wav", "/lib/kick.wav"]);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn sample_paths_before_fl_version_are_ignored_not_misdecoded() {
        // Malformed/unrealistic ordering (FLVersion should always be
        // first in a real file), but must not panic or produce garbage.
        let mut events = Vec::new();
        events.extend(text_event_utf16(EVENT_SAMPLE_PATH, "/lib/kick.wav"));
        events.extend(fl_version_event("21.2.1.3"));

        let path = temp_flp_path("premature_sample");
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(parse(&path).unwrap().sample_paths, Vec::<String>::new());

        fs::remove_file(&path).ok();
    }

    #[test]
    fn varint_round_trips_multi_byte_lengths() {
        for &n in &[0usize, 1, 127, 128, 300, 16384, 100_000] {
            let mut buf = Vec::new();
            write_varint(&mut buf, n);
            let (decoded, len) = read_varint(&buf).unwrap();
            assert_eq!(decoded, n);
            assert_eq!(len, buf.len());
        }
    }
}
