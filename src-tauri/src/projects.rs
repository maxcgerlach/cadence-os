use crate::db::DbState;
use crate::flp;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use tauri::State;
use walkdir::WalkDir;

const PROJECT_EXTENSIONS: &[&str] = &["flp"];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub tempo: Option<f64>,
    pub created_at: String,
}

pub(crate) struct DiscoveredProject {
    file_path: String,
    file_name: String,
    tempo: Option<f64>,
}

fn is_project_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PROJECT_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Walks `dir_path` for FL Studio project files and reads each one's tempo
/// directly out of the file's binary event stream (see `flp::read_tempo`)
/// — exact, not estimated. Reading a tempo event is cheap (no full decode
/// like audio analysis), so this runs sequentially rather than via rayon.
pub(crate) fn discover_projects(dir_path: &str) -> Vec<DiscoveredProject> {
    WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_project_file(entry.path()))
        .filter_map(|entry| {
            let path = entry.path();
            let file_path = path.to_str()?.to_string();
            let file_name = path.file_name()?.to_str()?.to_string();
            let tempo = flp::read_tempo(path);
            Some(DiscoveredProject {
                file_path,
                file_name,
                tempo,
            })
        })
        .collect()
}

pub(crate) fn insert_batch(conn: &Connection, projects: &[DiscoveredProject]) -> rusqlite::Result<usize> {
    let mut inserted = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO projects (file_path, file_name, tempo)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
                tempo = COALESCE(projects.tempo, excluded.tempo)
             WHERE projects.tempo IS NULL",
        )?;
        for p in projects {
            inserted += stmt.execute(params![p.file_path, p.file_name, p.tempo])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

fn list_projects_impl(conn: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, file_name, tempo, created_at FROM projects ORDER BY file_name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            file_path: row.get(1)?,
            file_name: row.get(2)?,
            tempo: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

#[tauri::command]
pub fn list_projects(db: State<'_, DbState>) -> Result<Vec<Project>, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    list_projects_impl(&conn).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                tempo REAL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        conn
    }

    fn project(path: &str, tempo: Option<f64>) -> DiscoveredProject {
        DiscoveredProject {
            file_path: path.to_string(),
            file_name: "track.flp".to_string(),
            tempo,
        }
    }

    #[test]
    fn inserts_new_project_with_tempo() {
        let conn = test_conn();
        let inserted = insert_batch(&conn, &[project("/projects/track.flp", Some(132.51))]).unwrap();
        assert_eq!(inserted, 1);

        let tempo: Option<f64> = conn
            .query_row("SELECT tempo FROM projects WHERE file_path = ?1", ["/projects/track.flp"], |r| r.get(0))
            .unwrap();
        assert_eq!(tempo, Some(132.51));
    }

    #[test]
    fn rescanning_same_path_does_not_duplicate() {
        let conn = test_conn();
        let files = vec![project("/projects/track.flp", Some(132.51))];
        insert_batch(&conn, &files).unwrap();
        insert_batch(&conn, &files).unwrap();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn rescan_backfills_tempo_that_was_previously_null() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/projects/track.flp", None)]).unwrap();
        let inserted = insert_batch(&conn, &[project("/projects/track.flp", Some(132.51))]).unwrap();
        assert_eq!(inserted, 1);

        let tempo: Option<f64> = conn
            .query_row("SELECT tempo FROM projects WHERE file_path = ?1", ["/projects/track.flp"], |r| r.get(0))
            .unwrap();
        assert_eq!(tempo, Some(132.51));
    }

    #[test]
    fn rescan_does_not_clobber_existing_tempo() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/projects/track.flp", Some(132.51))]).unwrap();
        let inserted = insert_batch(&conn, &[project("/projects/track.flp", None)]).unwrap();
        assert_eq!(inserted, 0);

        let tempo: Option<f64> = conn
            .query_row("SELECT tempo FROM projects WHERE file_path = ?1", ["/projects/track.flp"], |r| r.get(0))
            .unwrap();
        assert_eq!(tempo, Some(132.51));
    }

    #[test]
    fn list_projects_orders_by_name() {
        let conn = test_conn();
        insert_batch(
            &conn,
            &[
                DiscoveredProject { file_path: "/p/b.flp".into(), file_name: "b.flp".into(), tempo: Some(120.0) },
                DiscoveredProject { file_path: "/p/a.flp".into(), file_name: "a.flp".into(), tempo: Some(120.0) },
            ],
        )
        .unwrap();

        let projects = list_projects_impl(&conn).unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.file_name.as_str()).collect();
        assert_eq!(names, vec!["a.flp", "b.flp"]);
    }
}
