use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct DbState(pub Mutex<Connection>);

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

CREATE INDEX IF NOT EXISTS idx_samples_file_name ON samples(file_name);
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
