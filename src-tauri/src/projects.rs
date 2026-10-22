use crate::db::DbState;
use crate::flp;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;
use walkdir::WalkDir;

const PROJECT_EXTENSIONS: &[&str] = &["flp"];
const USER_DATA_TOKEN: &str = "%FLStudioUserData%";
const FACTORY_DATA_TOKEN: &str = "%FLStudioFactoryData%";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub tempo: Option<f64>,
    pub created_at: String,
    pub sample_count: i64,
}

/// One distinct sample file a project references (a sample used by many
/// channels is stored once).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SampleRef {
    raw_path: String,
    /// Real on-disk path with FL's `%FLStudioUserData%` token expanded.
    /// None for stock library samples, or if the FL user-data folder
    /// couldn't be located so a token path can't be resolved.
    resolved_path: Option<String>,
    file_name: String,
    is_factory: bool,
}

pub(crate) struct DiscoveredProject {
    file_path: String,
    file_name: String,
    tempo: Option<f64>,
    /// None when the .flp couldn't be parsed at all — in that case an
    /// existing sample list is left untouched rather than wiped (e.g. FL
    /// was mid-save when the watcher fired).
    samples: Option<Vec<SampleRef>>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ProjectSample {
    pub raw_path: String,
    pub resolved_path: Option<String>,
    pub file_name: String,
    /// "factory" (stock FL library), "found", "missing", or "unknown"
    /// (token path we couldn't resolve on this machine).
    pub status: String,
    /// Whether the sample lives inside the folder containing the project
    /// file. False is the "recorded somewhere else / in another project's
    /// folder" signal.
    pub in_project_folder: bool,
    /// Other locations that hold a file with this same name, across all
    /// projects and the indexed library. Empty = no name collision.
    pub duplicate_paths: Vec<String>,
    /// True when at least one of those other files has a different size
    /// than this one — i.e. it's most likely a *different* recording that
    /// happens to share a name (the dangerous case), rather than a plain
    /// copy of the same file (e.g. a template's kit copied into a project).
    pub duplicate_conflict: bool,
}

/// FL Studio writes autosaves and pre-overwrite snapshots into `Backup`
/// folders. They're near-copies of real projects and would bury them in
/// the list, so they aren't indexed.
fn is_in_backup_dir(path: &Path) -> bool {
    path.parent()
        .map(|dir| {
            dir.components()
                .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("backup"))
        })
        .unwrap_or(false)
}

fn is_project_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PROJECT_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// FL Studio's per-user data folder, which `%FLStudioUserData%` expands to.
pub(crate) fn default_user_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let dir = PathBuf::from(home).join("Documents/Image-Line/FL Studio");
    dir.is_dir().then_some(dir)
}

pub(crate) fn make_sample_ref(raw: &str, user_data_dir: Option<&Path>) -> Option<SampleRef> {
    // Paths come straight from the project and may use Windows separators
    // even when read on a Mac, so split on both.
    let file_name = raw.rsplit(['/', '\\']).next().unwrap_or(raw).to_string();
    if file_name.is_empty() {
        // e.g. a bare "%FLStudioFactoryData%/" folder reference.
        return None;
    }

    let is_factory = raw.contains(FACTORY_DATA_TOKEN);
    let resolved_path = if is_factory {
        None
    } else if let Some(rest) = raw.strip_prefix(USER_DATA_TOKEN) {
        user_data_dir.map(|dir| {
            let rest = rest.trim_start_matches(['/', '\\']);
            let rest = if cfg!(windows) { rest.to_string() } else { rest.replace('\\', "/") };
            dir.join(rest).to_string_lossy().into_owned()
        })
    } else {
        Some(raw.to_string())
    };

    Some(SampleRef {
        raw_path: raw.to_string(),
        resolved_path,
        file_name,
        is_factory,
    })
}

/// Walks `dir_path` for FL Studio project files and reads each one's tempo
/// and referenced samples directly out of the file's binary event stream
/// (see `flp::parse`) — exact, not estimated. Cheap (no audio decode), so
/// this runs sequentially rather than via rayon.
pub(crate) fn discover_projects(dir_path: &str) -> Vec<DiscoveredProject> {
    let user_data_dir = default_user_data_dir();

    WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_project_file(entry.path()) && !is_in_backup_dir(entry.path()))
        .filter_map(|entry| {
            let path = entry.path();
            let file_path = path.to_str()?.to_string();
            let file_name = path.file_name()?.to_str()?.to_string();
            let parsed = flp::parse(path);

            let tempo = parsed.as_ref().and_then(|p| p.tempo);
            let samples = parsed.map(|p| {
                let mut seen = std::collections::HashSet::new();
                p.sample_paths
                    .iter()
                    .filter(|raw| seen.insert((*raw).clone()))
                    .filter_map(|raw| make_sample_ref(raw, user_data_dir.as_deref()))
                    .collect()
            });

            Some(DiscoveredProject {
                file_path,
                file_name,
                tempo,
                samples,
            })
        })
        .collect()
}

pub(crate) fn insert_batch(conn: &Connection, projects: &[DiscoveredProject]) -> rusqlite::Result<usize> {
    let mut inserted = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO projects (file_path, file_name, tempo)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
                tempo = COALESCE(projects.tempo, excluded.tempo)
             WHERE projects.tempo IS NULL",
        )?;
        let mut insert_sample = tx.prepare(
            "INSERT OR IGNORE INTO project_samples (project_id, raw_path, resolved_path, file_name, is_factory)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        for p in projects {
            inserted += upsert.execute(params![p.file_path, p.file_name, p.tempo])?;

            // The project file is the source of truth for its sample list,
            // so replace it wholesale on every scan (but only if we could
            // actually read the file this time).
            if let Some(samples) = &p.samples {
                let project_id: i64 = tx.query_row(
                    "SELECT id FROM projects WHERE file_path = ?1",
                    params![p.file_path],
                    |r| r.get(0),
                )?;
                tx.execute("DELETE FROM project_samples WHERE project_id = ?1", params![project_id])?;
                for s in samples {
                    insert_sample.execute(params![
                        project_id,
                        s.raw_path,
                        s.resolved_path,
                        s.file_name,
                        s.is_factory
                    ])?;
                }
            }
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// Removes projects whose file no longer exists (deleted/moved) or sits in
/// a Backup folder (indexed before those were skipped), along with their
/// sample lists. Keeps the project view to what actually exists now.
pub(crate) fn prune_stale(conn: &Connection) -> rusqlite::Result<usize> {
    let rows = conn
        .prepare("SELECT id, file_path FROM projects")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut removed = 0;
    for (id, file_path) in rows {
        let path = Path::new(&file_path);
        if !path.exists() || is_in_backup_dir(path) {
            conn.execute("DELETE FROM project_samples WHERE project_id = ?1", params![id])?;
            conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn list_projects_impl(conn: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.file_path, p.file_name, p.tempo, p.created_at,
                (SELECT COUNT(*) FROM project_samples ps WHERE ps.project_id = p.id)
         FROM projects p
         ORDER BY p.file_name COLLATE NOCASE ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            file_path: row.get(1)?,
            file_name: row.get(2)?,
            tempo: row.get(3)?,
            created_at: row.get(4)?,
            sample_count: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// Other locations holding a file with the same name (case-insensitive),
/// across every project's user samples and the indexed library. Stock FL
/// samples are excluded — their names collide with user files constantly
/// and aren't the problem being solved here.
fn find_duplicate_paths(conn: &Connection, file_name: &str, own_path: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM (
             SELECT resolved_path AS path FROM project_samples
              WHERE is_factory = 0 AND resolved_path IS NOT NULL
                AND file_name = ?1 COLLATE NOCASE
             UNION
             SELECT file_path AS path FROM samples WHERE file_name = ?1 COLLATE NOCASE
         )
         WHERE path <> ?2
         ORDER BY path",
    )?;
    let rows = stmt.query_map(params![file_name, own_path], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Whether any of `others` is a file of a different size than `own`.
/// Files that can't be read (missing, other machine's drive) are not
/// comparable, so they never count as a conflict.
fn has_size_conflict(own: &str, others: &[String]) -> bool {
    let Ok(own_size) = std::fs::metadata(own).map(|m| m.len()) else {
        return false;
    };
    others
        .iter()
        .filter_map(|o| std::fs::metadata(o).ok().map(|m| m.len()))
        .any(|size| size != own_size)
}

fn list_project_samples_impl(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<ProjectSample>> {
    let project_file: String = conn.query_row(
        "SELECT file_path FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )?;
    let project_dir = Path::new(&project_file).parent().map(Path::to_path_buf);

    let mut stmt = conn.prepare(
        "SELECT raw_path, resolved_path, file_name, is_factory
         FROM project_samples WHERE project_id = ?1
         ORDER BY file_name COLLATE NOCASE ASC, raw_path ASC",
    )?;
    let refs = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut out = Vec::with_capacity(refs.len());
    for (raw_path, resolved_path, file_name, is_factory) in refs {
        // Existence is checked live (not stored) so it never goes stale
        // when files are moved or deleted after the project was scanned.
        let status = if is_factory {
            "factory"
        } else {
            match &resolved_path {
                None => "unknown",
                Some(p) if Path::new(p).exists() => "found",
                Some(_) => "missing",
            }
        };

        let in_project_folder = match (&resolved_path, &project_dir) {
            (Some(p), Some(dir)) => Path::new(p).starts_with(dir),
            _ => false,
        };

        let duplicate_paths = match (&resolved_path, is_factory) {
            (Some(own), false) => find_duplicate_paths(conn, &file_name, own)?,
            _ => Vec::new(),
        };
        let duplicate_conflict = match &resolved_path {
            Some(own) => has_size_conflict(own, &duplicate_paths),
            None => false,
        };

        out.push(ProjectSample {
            raw_path,
            resolved_path,
            file_name,
            status: status.to_string(),
            in_project_folder,
            duplicate_paths,
            duplicate_conflict,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn list_projects(db: State<'_, DbState>) -> Result<Vec<Project>, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    list_projects_impl(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_project_samples(db: State<'_, DbState>, project_id: i64) -> Result<Vec<ProjectSample>, String> {
    let conn = db.0.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    list_project_samples_impl(&conn, project_id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                tempo REAL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE project_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                raw_path TEXT NOT NULL,
                resolved_path TEXT,
                file_name TEXT NOT NULL,
                is_factory INTEGER NOT NULL DEFAULT 0,
                UNIQUE(project_id, raw_path)
            );
            CREATE TABLE samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn sample_ref(resolved: &str) -> SampleRef {
        make_sample_ref(resolved, None).unwrap()
    }

    fn project(path: &str, tempo: Option<f64>, samples: Option<Vec<SampleRef>>) -> DiscoveredProject {
        DiscoveredProject {
            file_path: path.to_string(),
            file_name: Path::new(path).file_name().unwrap().to_str().unwrap().to_string(),
            tempo,
            samples,
        }
    }

    fn project_id(conn: &Connection, path: &str) -> i64 {
        conn.query_row("SELECT id FROM projects WHERE file_path = ?1", [path], |r| r.get(0)).unwrap()
    }

    // --- make_sample_ref ---

    #[test]
    fn factory_samples_have_no_resolved_path() {
        let s = make_sample_ref("%FLStudioFactoryData%/Data/Patches/Packs/Drums/Kicks/Monster Kick 090.wav", None).unwrap();
        assert!(s.is_factory);
        assert_eq!(s.resolved_path, None);
        assert_eq!(s.file_name, "Monster Kick 090.wav");
    }

    #[test]
    fn user_data_token_with_windows_separators_resolves_on_this_machine() {
        let dir = Path::new("/Users/me/Documents/Image-Line/FL Studio");
        let s = make_sample_ref(
            "%FLStudioUserData%\\Audio\\Recorded\\untitled_2026-04-02 21-10-06_Insert 2.wav",
            Some(dir),
        )
        .unwrap();
        assert!(!s.is_factory);
        assert_eq!(s.file_name, "untitled_2026-04-02 21-10-06_Insert 2.wav");
        if cfg!(not(windows)) {
            assert_eq!(
                s.resolved_path.as_deref(),
                Some("/Users/me/Documents/Image-Line/FL Studio/Audio/Recorded/untitled_2026-04-02 21-10-06_Insert 2.wav")
            );
        }
    }

    #[test]
    fn user_data_token_with_forward_slashes_resolves() {
        let dir = Path::new("/Users/me/Documents/Image-Line/FL Studio");
        let s = make_sample_ref("%FLStudioUserData%/Projects/miles above/Samples/kick.wav", Some(dir)).unwrap();
        assert_eq!(
            s.resolved_path.as_deref(),
            Some("/Users/me/Documents/Image-Line/FL Studio/Projects/miles above/Samples/kick.wav")
        );
    }

    #[test]
    fn user_data_token_without_known_user_dir_is_unresolved_not_missing() {
        let s = make_sample_ref("%FLStudioUserData%/Audio/x.wav", None).unwrap();
        assert_eq!(s.resolved_path, None);
        assert!(!s.is_factory);
    }

    #[test]
    fn plain_windows_drive_path_is_kept_as_is_and_file_name_extracted() {
        let s = make_sample_ref("D:\\1! Kits\\Snares\\Snare (SeX).wav", None).unwrap();
        assert_eq!(s.file_name, "Snare (SeX).wav");
        assert_eq!(s.resolved_path.as_deref(), Some("D:\\1! Kits\\Snares\\Snare (SeX).wav"));
    }

    #[test]
    fn bare_folder_reference_is_skipped() {
        assert!(make_sample_ref("%FLStudioFactoryData%/", None).is_none());
    }

    // --- insert_batch / project rows ---

    #[test]
    fn inserts_new_project_with_tempo() {
        let conn = test_conn();
        let inserted = insert_batch(&conn, &[project("/projects/track.flp", Some(132.51), None)]).unwrap();
        assert_eq!(inserted, 1);

        let tempo: Option<f64> = conn
            .query_row("SELECT tempo FROM projects WHERE file_path = ?1", ["/projects/track.flp"], |r| r.get(0))
            .unwrap();
        assert_eq!(tempo, Some(132.51));
    }

    #[test]
    fn rescan_backfills_tempo_but_never_clobbers_it() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/p/a.flp", None, None)]).unwrap();
        assert_eq!(insert_batch(&conn, &[project("/p/a.flp", Some(120.0), None)]).unwrap(), 1);
        assert_eq!(insert_batch(&conn, &[project("/p/a.flp", None, None)]).unwrap(), 0);

        let tempo: Option<f64> = conn.query_row("SELECT tempo FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(tempo, Some(120.0));
    }

    // --- project_samples storage ---

    #[test]
    fn stores_samples_and_dedupes_repeated_references() {
        let conn = test_conn();
        let samples = vec![sample_ref("/lib/kick.wav"), sample_ref("/lib/kick.wav"), sample_ref("/lib/snare.wav")];
        insert_batch(&conn, &[project("/p/a.flp", None, Some(samples))]).unwrap();

        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_samples", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn rescan_replaces_the_sample_list() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/p/a.flp", None, Some(vec![sample_ref("/lib/old.wav")]))]).unwrap();
        insert_batch(&conn, &[project("/p/a.flp", None, Some(vec![sample_ref("/lib/new.wav")]))]).unwrap();

        let names: Vec<String> = conn
            .prepare("SELECT file_name FROM project_samples")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(names, vec!["new.wav"]);
    }

    #[test]
    fn unparseable_rescan_keeps_existing_sample_list() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/p/a.flp", None, Some(vec![sample_ref("/lib/kick.wav")]))]).unwrap();
        insert_batch(&conn, &[project("/p/a.flp", None, None)]).unwrap();

        let n: i64 = conn.query_row("SELECT COUNT(*) FROM project_samples", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn list_projects_reports_sample_count_and_orders_by_name() {
        let conn = test_conn();
        insert_batch(
            &conn,
            &[
                project("/p/b.flp", None, Some(vec![sample_ref("/lib/1.wav"), sample_ref("/lib/2.wav")])),
                project("/p/A.flp", None, Some(vec![])),
            ],
        )
        .unwrap();

        let projects = list_projects_impl(&conn).unwrap();
        let summary: Vec<(&str, i64)> = projects.iter().map(|p| (p.file_name.as_str(), p.sample_count)).collect();
        assert_eq!(summary, vec![("A.flp", 0), ("b.flp", 2)]);
    }

    // --- list_project_samples ---

    #[test]
    fn status_reflects_whether_the_file_exists_right_now() {
        let dir = std::env::temp_dir().join(format!("cadence_ps_status_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let real = dir.join("here.wav");
        fs::write(&real, b"x").unwrap();
        let gone = dir.join("gone.wav");

        let conn = test_conn();
        let proj = dir.join("song.flp");
        insert_batch(
            &conn,
            &[project(
                proj.to_str().unwrap(),
                None,
                Some(vec![
                    sample_ref(real.to_str().unwrap()),
                    sample_ref(gone.to_str().unwrap()),
                    make_sample_ref("%FLStudioFactoryData%/Kicks/k.wav", None).unwrap(),
                    make_sample_ref("%FLStudioUserData%/x.wav", None).unwrap(),
                ]),
            )],
        )
        .unwrap();

        let id = project_id(&conn, proj.to_str().unwrap());
        let samples = list_project_samples_impl(&conn, id).unwrap();
        let status = |name: &str| samples.iter().find(|s| s.file_name == name).unwrap().status.clone();
        assert_eq!(status("here.wav"), "found");
        assert_eq!(status("gone.wav"), "missing");
        assert_eq!(status("k.wav"), "factory");
        assert_eq!(status("x.wav"), "unknown");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flags_samples_stored_outside_the_projects_own_folder() {
        let conn = test_conn();
        insert_batch(
            &conn,
            &[project(
                "/music/song/song.flp",
                None,
                Some(vec![
                    sample_ref("/music/song/Samples/kick.wav"),
                    sample_ref("/music/other project/Audio/untitled_rec.wav"),
                ]),
            )],
        )
        .unwrap();

        let id = project_id(&conn, "/music/song/song.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        let inside = |name: &str| samples.iter().find(|s| s.file_name == name).unwrap().in_project_folder;
        assert!(inside("kick.wav"));
        assert!(!inside("untitled_rec.wav"));
    }

    #[test]
    fn detects_same_name_in_a_different_project() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/m/a/a.flp", None, Some(vec![sample_ref("/m/a/Audio/untitled.wav")]))]).unwrap();
        insert_batch(&conn, &[project("/m/b/b.flp", None, Some(vec![sample_ref("/m/b/Audio/untitled.wav")]))]).unwrap();

        let id = project_id(&conn, "/m/a/a.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        assert_eq!(samples[0].duplicate_paths, vec!["/m/b/Audio/untitled.wav"]);
    }

    #[test]
    fn detects_same_name_in_the_indexed_library_case_insensitively() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO samples (file_path, file_name) VALUES ('/lib/Untitled.WAV', 'Untitled.WAV')",
            [],
        )
        .unwrap();
        insert_batch(&conn, &[project("/m/a/a.flp", None, Some(vec![sample_ref("/m/a/Audio/untitled.wav")]))]).unwrap();

        let id = project_id(&conn, "/m/a/a.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        assert_eq!(samples[0].duplicate_paths, vec!["/lib/Untitled.WAV"]);
    }

    #[test]
    fn same_file_used_by_two_projects_is_not_a_duplicate() {
        let conn = test_conn();
        insert_batch(&conn, &[project("/m/a/a.flp", None, Some(vec![sample_ref("/lib/kick.wav")]))]).unwrap();
        insert_batch(&conn, &[project("/m/b/b.flp", None, Some(vec![sample_ref("/lib/kick.wav")]))]).unwrap();

        let id = project_id(&conn, "/m/a/a.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        assert!(samples[0].duplicate_paths.is_empty());
    }

    #[test]
    fn factory_samples_never_count_as_duplicates() {
        let conn = test_conn();
        let factory = make_sample_ref("%FLStudioFactoryData%/Kicks/kick.wav", None).unwrap();
        insert_batch(&conn, &[project("/m/a/a.flp", None, Some(vec![factory.clone()]))]).unwrap();
        insert_batch(&conn, &[project("/m/b/b.flp", None, Some(vec![sample_ref("/lib/kick.wav"), factory]))]).unwrap();

        let id = project_id(&conn, "/m/b/b.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        assert!(samples.iter().all(|s| s.duplicate_paths.is_empty()));
    }

    // --- backup folders ---

    #[test]
    fn backup_directories_are_recognised_case_insensitively() {
        assert!(is_in_backup_dir(Path::new("/m/Projects/Backup/song (autosaved at 10h).flp")));
        assert!(is_in_backup_dir(Path::new("/m/Projects/backup/song.flp")));
        assert!(!is_in_backup_dir(Path::new("/m/Projects/song/song.flp")));
        // A project merely *named* like a backup file isn't in a Backup dir.
        assert!(!is_in_backup_dir(Path::new("/m/Projects/Backup song.flp")));
    }

    #[test]
    fn discover_projects_skips_backup_folders() {
        let dir = std::env::temp_dir().join(format!("cadence_discover_proj_{}", std::process::id()));
        fs::create_dir_all(dir.join("Backup")).unwrap();
        fs::write(dir.join("real.flp"), b"x").unwrap();
        fs::write(dir.join("Backup").join("real (autosaved at 10h).flp"), b"x").unwrap();

        let found = discover_projects(dir.to_str().unwrap());
        let names: Vec<&str> = found.iter().map(|p| p.file_name.as_str()).collect();
        assert_eq!(names, vec!["real.flp"]);

        fs::remove_dir_all(&dir).ok();
    }

    // --- size-based conflict ---

    #[test]
    fn same_name_different_size_is_a_conflict_same_size_is_a_copy() {
        let dir = std::env::temp_dir().join(format!("cadence_conflict_{}", std::process::id()));
        for sub in ["a", "b", "c"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        fs::write(dir.join("a/take.wav"), b"1234").unwrap();
        fs::write(dir.join("b/take.wav"), b"123456789").unwrap(); // different recording
        fs::write(dir.join("c/take.wav"), b"1234").unwrap(); // identical-size copy

        let a = dir.join("a/take.wav").to_string_lossy().into_owned();
        let b = dir.join("b/take.wav").to_string_lossy().into_owned();
        let c = dir.join("c/take.wav").to_string_lossy().into_owned();

        assert!(has_size_conflict(&a, &[b.clone()]));
        assert!(!has_size_conflict(&a, &[c.clone()]));
        assert!(has_size_conflict(&a, &[c, b]));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_files_never_count_as_conflicts() {
        assert!(!has_size_conflict("/nope/own.wav", &["/nope/other.wav".to_string()]));
        let dir = std::env::temp_dir().join(format!("cadence_conflict_unreadable_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("own.wav"), b"1234").unwrap();
        let own = dir.join("own.wav").to_string_lossy().into_owned();
        assert!(!has_size_conflict(&own, &["/nope/other.wav".to_string()]));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_project_samples_reports_conflict_for_real_files() {
        let dir = std::env::temp_dir().join(format!("cadence_conflict_e2e_{}", std::process::id()));
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        fs::write(dir.join("a/untitled.wav"), b"first recording").unwrap();
        fs::write(dir.join("b/untitled.wav"), b"a totally different one").unwrap();

        let conn = test_conn();
        let a = dir.join("a/untitled.wav").to_string_lossy().into_owned();
        let b = dir.join("b/untitled.wav").to_string_lossy().into_owned();
        insert_batch(&conn, &[project("/m/a/a.flp", None, Some(vec![sample_ref(&a)]))]).unwrap();
        insert_batch(&conn, &[project("/m/b/b.flp", None, Some(vec![sample_ref(&b)]))]).unwrap();

        let id = project_id(&conn, "/m/a/a.flp");
        let samples = list_project_samples_impl(&conn, id).unwrap();
        assert_eq!(samples[0].duplicate_paths, vec![b]);
        assert!(samples[0].duplicate_conflict);

        fs::remove_dir_all(&dir).ok();
    }

    // --- pruning ---

    #[test]
    fn prune_removes_deleted_and_backup_projects_with_their_samples_but_keeps_live_ones() {
        let dir = std::env::temp_dir().join(format!("cadence_prune_{}", std::process::id()));
        fs::create_dir_all(dir.join("Backup")).unwrap();
        let live = dir.join("live.flp");
        let backup = dir.join("Backup").join("live (autosaved at 10h).flp");
        let gone = dir.join("gone.flp");
        fs::write(&live, b"x").unwrap();
        fs::write(&backup, b"x").unwrap(); // exists, but is a backup

        let conn = test_conn();
        for p in [&live, &backup, &gone] {
            insert_batch(&conn, &[project(p.to_str().unwrap(), None, Some(vec![sample_ref("/lib/k.wav")]))]).unwrap();
        }

        assert_eq!(prune_stale(&conn).unwrap(), 2);

        let remaining: Vec<String> = list_projects_impl(&conn).unwrap().into_iter().map(|p| p.file_name).collect();
        assert_eq!(remaining, vec!["live.flp"]);
        let orphaned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM project_samples WHERE project_id NOT IN (SELECT id FROM projects)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphaned, 0);

        fs::remove_dir_all(&dir).ok();
    }
}
