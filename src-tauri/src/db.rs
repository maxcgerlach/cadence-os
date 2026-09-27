use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub struct DbState(pub Arc<Mutex<Connection>>);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path   TEXT NOT NULL UNIQUE,
    file_name   TEXT NOT NULL,
    extension   TEXT NOT NULL,
    duration_ms INTEGER,
    sample_rate INTEGER,
    channels    INTEGER,
    bpm         REAL,
    pitch_key   TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS virtual_tags (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    color_hex  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sample_tags (
    sample_id  INTEGER NOT NULL REFERENCES samples(id) ON DELETE CASCADE,
    tag_id     INTEGER NOT NULL REFERENCES virtual_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (sample_id, tag_id)
);

CREATE TABLE IF NOT EXISTS watched_folders (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    path       TEXT NOT NULL UNIQUE,
    added_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path  TEXT NOT NULL UNIQUE,
    file_name  TEXT NOT NULL,
    tempo      REAL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_samples_file_name ON samples(file_name);
CREATE INDEX IF NOT EXISTS idx_projects_file_name ON projects(file_name);
";

/// Opens (creating if needed) `cadence.db` in the app's data directory and
/// applies the schema. Called once from `setup` and stashed in managed state.
pub fn init(app: &AppHandle) -> Connection {
    let data_dir = app
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");

    std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");

    let db_path = data_dir.join("cadence.db");
    let conn = Connection::open(db_path).expect("failed to open cadence.db");

    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("failed to set WAL mode");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("failed to enable foreign keys");

    conn.execute_batch(SCHEMA).expect("failed to apply schema");

    conn
}

/// Records a folder as watched (idempotent — re-scanning an already-watched
/// folder is a no-op here).
pub fn add_watched_folder(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO watched_folders (path) VALUES (?1)",
        params![path],
    )?;
    Ok(())
}

pub fn list_watched_folders(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM watched_folders ORDER BY path ASC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    #[test]
    fn add_watched_folder_is_idempotent() {
        let conn = test_conn();
        add_watched_folder(&conn, "/library/kicks").unwrap();
        add_watched_folder(&conn, "/library/kicks").unwrap();

        let folders = list_watched_folders(&conn).unwrap();
        assert_eq!(folders, vec!["/library/kicks".to_string()]);
    }

    #[test]
    fn lists_watched_folders_sorted() {
        let conn = test_conn();
        add_watched_folder(&conn, "/library/snares").unwrap();
        add_watched_folder(&conn, "/library/kicks").unwrap();

        let folders = list_watched_folders(&conn).unwrap();
        assert_eq!(folders, vec!["/library/kicks".to_string(), "/library/snares".to_string()]);
    }
}
