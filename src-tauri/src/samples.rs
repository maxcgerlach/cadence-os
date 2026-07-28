use crate::db::DbState;
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Sample {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub extension: String,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub bpm: Option<f64>,
    pub pitch_key: Option<String>,
    pub created_at: String,
    pub tag_ids: Vec<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct VirtualTag {
    pub id: i64,
    pub name: String,
    pub color_hex: String,
}

fn parse_tag_ids(raw: Option<String>) -> Vec<i64> {
    raw.map(|s| s.split(',').filter_map(|id| id.parse().ok()).collect())
        .unwrap_or_default()
}

/// Returns all indexed samples, optionally filtered by a case-insensitive
/// substring match on file_name. Each sample's assigned tag IDs are pulled
/// in via a correlated GROUP_CONCAT subquery to avoid N+1 queries.
fn list_samples_impl(conn: &Connection, search: Option<&str>) -> rusqlite::Result<Vec<Sample>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.file_path, s.file_name, s.extension, s.duration_ms, s.sample_rate,
                s.channels, s.bpm, s.pitch_key, s.created_at,
                (SELECT GROUP_CONCAT(tag_id) FROM sample_tags WHERE sample_id = s.id) AS tag_ids
         FROM samples s
         WHERE (?1 IS NULL OR s.file_name LIKE '%' || ?1 || '%' COLLATE NOCASE)
         ORDER BY s.file_name ASC",
    )?;

    let rows = stmt.query_map(params![search], |row| {
        Ok(Sample {
            id: row.get(0)?,
            file_path: row.get(1)?,
            file_name: row.get(2)?,
            extension: row.get(3)?,
            duration_ms: row.get(4)?,
            sample_rate: row.get(5)?,
            channels: row.get(6)?,
            bpm: row.get(7)?,
            pitch_key: row.get(8)?,
            created_at: row.get(9)?,
            tag_ids: parse_tag_ids(row.get(10)?),
        })
    })?;

    rows.collect()
}

fn list_virtual_tags_impl(conn: &Connection) -> rusqlite::Result<Vec<VirtualTag>> {
    let mut stmt = conn.prepare("SELECT id, name, color_hex FROM virtual_tags ORDER BY name ASC")?;

    let rows = stmt.query_map([], |row| {
        Ok(VirtualTag {
            id: row.get(0)?,
            name: row.get(1)?,
            color_hex: row.get(2)?,
        })
    })?;

    rows.collect()
}

/// Creates a new virtual tag. Fails with a friendly message if the name is
/// already taken (virtual_tags.name is UNIQUE).
fn create_tag_impl(conn: &Connection, name: &str, color_hex: &str) -> Result<VirtualTag, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Tag name cannot be empty".to_string());
    }

    conn.execute(
        "INSERT INTO virtual_tags (name, color_hex) VALUES (?1, ?2)",
        params![name, color_hex],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(err, _) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
            format!("A tag named \"{name}\" already exists")
        }
        e => e.to_string(),
    })?;

    Ok(VirtualTag {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        color_hex: color_hex.to_string(),
    })
}

fn assign_tag_impl(conn: &Connection, sample_id: i64, tag_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sample_tags (sample_id, tag_id) VALUES (?1, ?2)",
        params![sample_id, tag_id],
    )?;
    Ok(())
}

fn remove_tag_impl(conn: &Connection, sample_id: i64, tag_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM sample_tags WHERE sample_id = ?1 AND tag_id = ?2",
        params![sample_id, tag_id],
    )?;
    Ok(())
}

#[tauri::command]
pub fn list_samples(db: State<'_, DbState>, search: Option<String>) -> Result<Vec<Sample>, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    list_samples_impl(&conn, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_virtual_tags(db: State<'_, DbState>) -> Result<Vec<VirtualTag>, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    list_virtual_tags_impl(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_tag(db: State<'_, DbState>, name: String, color_hex: String) -> Result<VirtualTag, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    create_tag_impl(&conn, &name, &color_hex)
}

#[tauri::command]
pub fn assign_tag(db: State<'_, DbState>, sample_id: i64, tag_id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    assign_tag_impl(&conn, sample_id, tag_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_tag(db: State<'_, DbState>, sample_id: i64, tag_id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    remove_tag_impl(&conn, sample_id, tag_id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            );
            CREATE TABLE virtual_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                color_hex TEXT NOT NULL
            );
            CREATE TABLE sample_tags (
                sample_id INTEGER NOT NULL REFERENCES samples(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES virtual_tags(id) ON DELETE CASCADE,
                PRIMARY KEY (sample_id, tag_id)
            );",
        )
        .unwrap();
        conn
    }

    fn seed_sample(conn: &Connection, file_path: &str, file_name: &str) -> i64 {
        conn.execute(
            "INSERT INTO samples (file_path, file_name, extension) VALUES (?1, ?2, 'wav')",
            params![file_path, file_name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn parses_comma_separated_tag_ids() {
        assert_eq!(parse_tag_ids(Some("1,2,3".to_string())), vec![1, 2, 3]);
        assert_eq!(parse_tag_ids(None), Vec::<i64>::new());
        assert_eq!(parse_tag_ids(Some("".to_string())), Vec::<i64>::new());
    }

    #[test]
    fn create_tag_then_list_round_trips() {
        let conn = test_conn();
        let created = create_tag_impl(&conn, "Kicks", "#ff0000").unwrap();
        assert_eq!(created.name, "Kicks");

        let tags = list_virtual_tags_impl(&conn).unwrap();
        assert_eq!(tags, vec![created]);
    }

    #[test]
    fn create_tag_rejects_duplicate_name_with_friendly_error() {
        let conn = test_conn();
        create_tag_impl(&conn, "Kicks", "#ff0000").unwrap();
        let err = create_tag_impl(&conn, "Kicks", "#00ff00").unwrap_err();
        assert!(err.contains("already exists"), "unexpected error: {err}");
    }

    #[test]
    fn create_tag_rejects_empty_name() {
        let conn = test_conn();
        let err = create_tag_impl(&conn, "   ", "#ff0000").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn assign_and_remove_tag_updates_list_samples_tag_ids() {
        let conn = test_conn();
        let sample_id = seed_sample(&conn, "/lib/kick.wav", "kick.wav");
        let tag = create_tag_impl(&conn, "Kicks", "#ff0000").unwrap();

        assign_tag_impl(&conn, sample_id, tag.id).unwrap();
        let samples = list_samples_impl(&conn, None).unwrap();
        assert_eq!(samples[0].tag_ids, vec![tag.id]);

        remove_tag_impl(&conn, sample_id, tag.id).unwrap();
        let samples = list_samples_impl(&conn, None).unwrap();
        assert_eq!(samples[0].tag_ids, Vec::<i64>::new());
    }

    #[test]
    fn assign_tag_is_idempotent() {
        let conn = test_conn();
        let sample_id = seed_sample(&conn, "/lib/kick.wav", "kick.wav");
        let tag = create_tag_impl(&conn, "Kicks", "#ff0000").unwrap();

        assign_tag_impl(&conn, sample_id, tag.id).unwrap();
        assign_tag_impl(&conn, sample_id, tag.id).unwrap();

        let samples = list_samples_impl(&conn, None).unwrap();
        assert_eq!(samples[0].tag_ids, vec![tag.id]);
    }

    #[test]
    fn list_samples_search_filters_by_name_case_insensitively() {
        let conn = test_conn();
        seed_sample(&conn, "/lib/Kick_808.wav", "Kick_808.wav");
        seed_sample(&conn, "/lib/snare.wav", "snare.wav");

        let results = list_samples_impl(&conn, Some("kick")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "Kick_808.wav");
    }
}
