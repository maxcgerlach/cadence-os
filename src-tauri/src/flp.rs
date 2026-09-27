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

/// Reads the project tempo directly out of an FL Studio `.flp` file's
/// binary event stream — this is the project's actual stored tempo, not an
/// estimate, so it's exact where audio-based BPM detection on a rendered
/// export can only guess.
///
/// Known limitation, verified against real files (not hypothetical): FL
/// Studio 25 projects containing certain newer VST plugin events can
/// desync the event walk partway through the file and yield None even
/// though the file has a tempo. This isn't unique to this implementation —
/// the reference `pyflp` parser (the most complete public FLP parser)
/// hits the exact same "Unknown VST marker" issue on the same files as of
/// 2026. Returning None here is the correct, honest failure mode rather
/// than guessing; a wrong-but-present number would be worse. FL Studio 21
/// projects (and presumably other pre-25 versions) parse correctly.
pub fn read_tempo(path: &Path) -> Option<f64> {
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
            // complex file must not discard a tempo value already found
            // earlier in the stream — stop walking, but keep what we have.
            let Some((size, varint_len)) = read_varint(&data[pos..end]) else {
                break;
            };
            pos += varint_len;
            if pos + size > end {
                break;
            }
            pos += size;
        }
    }

    if let Some(value) = tempo {
        return Some(value as f64 / 1000.0);
    }
    if let Some(coarse) = tempo_coarse {
        let fine = tempo_fine.unwrap_or(0) as f64 / 1000.0;
        return Some(coarse as f64 + fine);
    }
    None
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

    fn text_event(id: u8, text: &[u8]) -> Vec<u8> {
        let mut e = vec![id];
        write_varint(&mut e, text.len());
        e.extend_from_slice(text);
        e
    }

    fn write_flp_file(path: &Path, bytes: &[u8]) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    #[test]
    fn reads_modern_tempo_event() {
        let mut events = Vec::new();
        events.extend(text_event(194, b"some title")); // Title = TEXT+2, unrelated event before
        events.extend(dword_event(EVENT_TEMPO, 132_510)); // 132.51 BPM
        events.extend(dword_event(159, 20)); // FLBuild = DWORD+31, unrelated, after

        let path = std::env::temp_dir().join(format!("cadence_flp_modern_{}.flp", std::process::id()));
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(read_tempo(&path), Some(132.51));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn falls_back_to_legacy_coarse_and_fine_tempo_events() {
        let mut events = Vec::new();
        events.extend(word_event(EVENT_TEMPO_COARSE, 140));
        events.extend(word_event(EVENT_TEMPO_FINE, 500));

        let path = std::env::temp_dir().join(format!("cadence_flp_legacy_{}.flp", std::process::id()));
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(read_tempo(&path), Some(140.5));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn legacy_coarse_only_with_no_fine_event() {
        let events = word_event(EVENT_TEMPO_COARSE, 128);

        let path = std::env::temp_dir().join(format!("cadence_flp_coarse_only_{}.flp", std::process::id()));
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(read_tempo(&path), Some(128.0));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn modern_event_takes_precedence_over_legacy_ones() {
        let mut events = Vec::new();
        events.extend(word_event(EVENT_TEMPO_COARSE, 90));
        events.extend(dword_event(EVENT_TEMPO, 174_000));

        let path = std::env::temp_dir().join(format!("cadence_flp_precedence_{}.flp", std::process::id()));
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(read_tempo(&path), Some(174.0));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_none_for_missing_tempo_event() {
        let events = text_event(194, b"no tempo here"); // Title = TEXT+2

        let path = std::env::temp_dir().join(format!("cadence_flp_none_{}.flp", std::process::id()));
        write_flp_file(&path, &build_flp(&events));

        assert_eq!(read_tempo(&path), None);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_none_for_garbage_file() {
        let path = std::env::temp_dir().join(format!("cadence_flp_garbage_{}.flp", std::process::id()));
        write_flp_file(&path, b"not an flp file at all");

        assert_eq!(read_tempo(&path), None);

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
