use crate::db::DbState;
use crate::metadata;
use rayon::prelude::*;
use rusqlite::params;
use serde::Serialize;
use std::path::Path;
use tauri::State;
use walkdir::WalkDir;

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac"];

#[derive(Serialize, Clone)]
pub struct ScanResult {
    pub scanned: usize,
    pub inserted: usize,
}

struct DiscoveredPath {
    file_path: String,
    file_name: String,
    extension: String,
}

struct DiscoveredFile {
    file_path: String,
    file_name: String,
    extension: String,
    duration_ms: Option<i64>,
    sample_rate: Option<i64>,
    channels: Option<i64>,
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Walks `dir_path` recursively looking for .wav/.mp3/.flac files and inserts
/// any new ones into SQLite in batched transactions. Runs on a blocking
/// thread so the UI stays responsive while large libraries are indexed.
#[tauri::command]
pub async fn scan_directory(
    dir_path: String,
    db: State<'_, DbState>,
) -> Result<ScanResult, String> {
    // Walk the filesystem (blocking, IO bound) off the async runtime, then
    // probe each file's audio header in parallel with rayon — header probing
    // only reads the first chunk of each file, so this is CPU/IO bound per
    // file rather than a full decode, and scales well across cores.
    let discovered = tokio::task::spawn_blocking(move || {
        let paths: Vec<DiscoveredPath> = WalkDir::new(&dir_path)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| is_audio_file(entry.path()))
            .filter_map(|entry| {
                let path = entry.path();
                let file_path = path.to_str()?.to_string();
                let file_name = path.file_name()?.to_str()?.to_string();
                let extension = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                Some(DiscoveredPath {
                    file_path,
                    file_name,
                    extension,
                })
            })
            .collect();

        paths
            .into_par_iter()
            .map(|p| {
                let probed = metadata::extract(Path::new(&p.file_path));
                DiscoveredFile {
                    file_path: p.file_path,
                    file_name: p.file_name,
                    extension: p.extension,
                    duration_ms: probed.as_ref().map(|m| m.duration_ms),
                    sample_rate: probed.as_ref().map(|m| m.sample_rate),
                    channels: probed.as_ref().map(|m| m.channels),
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("scan task failed: {e}"))?;

    let scanned = discovered.len();

    // Batch-insert everything inside a single transaction for speed, and
    // hold the mutex only for the duration of the DB work.
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    let inserted = insert_batch(&conn, &discovered).map_err(|e| e.to_string())?;

    Ok(ScanResult { scanned, inserted })
}

fn insert_batch(
    conn: &rusqlite::Connection,
    files: &[DiscoveredFile],
) -> rusqlite::Result<usize> {
    let mut inserted = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO samples (file_path, file_name, extension, duration_ms, sample_rate, channels)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(file_path) DO UPDATE SET
                duration_ms = excluded.duration_ms,
                sample_rate = excluded.sample_rate,
                channels = excluded.channels
             WHERE samples.duration_ms IS NULL",
        )?;
        for file in files {
            inserted += stmt.execute(params![
                file.file_path,
                file.file_name,
                file.extension,
                file.duration_ms,
                file.sample_rate,
                file.channels,
            ])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                extension TEXT NOT NULL,
                duration_ms INTEGER,
                sample_rate INTEGER,
                channels INTEGER,
                bpm REAL,
                pitch_key TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        conn
    }

    fn kick(path: &str, duration_ms: Option<i64>) -> DiscoveredFile {
        DiscoveredFile {
            file_path: path.to_string(),
            file_name: "kick.wav".to_string(),
            extension: "wav".to_string(),
            duration_ms,
            sample_rate: duration_ms.map(|_| 44100),
            channels: duration_ms.map(|_| 1),
        }
    }

    #[test]
    fn inserts_new_files_with_metadata() {
        let conn = test_conn();
        let files = vec![kick("/library/kick.wav", Some(500))];

        let inserted = insert_batch(&conn, &files).unwrap();
        assert_eq!(inserted, 1);

        let duration: i64 = conn
            .query_row("SELECT duration_ms FROM samples WHERE file_path = ?1", ["/library/kick.wav"], |r| r.get(0))
            .unwrap();
        assert_eq!(duration, 500);
    }

    #[test]
    fn rescanning_same_path_does_not_duplicate() {
        let conn = test_conn();
        let files = vec![kick("/library/kick.wav", Some(500))];
        insert_batch(&conn, &files).unwrap();
        insert_batch(&conn, &files).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn rescan_backfills_metadata_that_was_previously_null() {
        let conn = test_conn();
        // First scan: metadata extraction failed (e.g. unreadable header).
        insert_batch(&conn, &[kick("/library/kick.wav", None)]).unwrap();
        // Second scan: file is now readable and yields real metadata.
        let inserted = insert_batch(&conn, &[kick("/library/kick.wav", Some(500))]).unwrap();
        assert_eq!(inserted, 1, "backfill update should count as affected");

        let duration: Option<i64> = conn
            .query_row("SELECT duration_ms FROM samples WHERE file_path = ?1", ["/library/kick.wav"], |r| r.get(0))
            .unwrap();
        assert_eq!(duration, Some(500));
    }

    #[test]
    fn rescan_does_not_clobber_existing_metadata() {
        let conn = test_conn();
        insert_batch(&conn, &[kick("/library/kick.wav", Some(500))]).unwrap();
        // Re-scan reports no metadata this time (shouldn't happen in practice,
        // but the WHERE clause should still protect existing good data).
        let inserted = insert_batch(&conn, &[kick("/library/kick.wav", None)]).unwrap();
        assert_eq!(inserted, 0);

        let duration: Option<i64> = conn
            .query_row("SELECT duration_ms FROM samples WHERE file_path = ?1", ["/library/kick.wav"], |r| r.get(0))
            .unwrap();
        assert_eq!(duration, Some(500));
    }
}
