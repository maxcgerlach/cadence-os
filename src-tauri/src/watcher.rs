use crate::{db, indexer};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, Debouncer};
use rusqlite::Connection;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Debounce window for filesystem events. A drag-and-drop of many files
/// fires one event per file; this collapses that burst into a single
/// re-scan instead of one per file.
const DEBOUNCE: Duration = Duration::from_secs(2);

pub const SAMPLES_UPDATED_EVENT: &str = "samples-updated";

pub struct WatcherState(Mutex<Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>>);

impl WatcherState {
    /// Adds a folder to the live watch set. Safe to call for a folder
    /// that's already watched — `notify` just re-registers it.
    pub fn watch(&self, path: &str) {
        if let Ok(mut debouncer) = self.0.lock() {
            let _ = debouncer
                .watcher()
                .watch(Path::new(path), RecursiveMode::Recursive);
        }
    }
}

/// Starts watching every previously-indexed folder for filesystem changes.
/// On any change, re-scans all watched folders (cheap: unchanged files are
/// no-ops via the same upsert `scan_directory` uses) and emits
/// `SAMPLES_UPDATED_EVENT` so the frontend can refetch without the user
/// having to manually re-scan.
pub fn init(app_handle: AppHandle, conn: Arc<Mutex<Connection>>) -> WatcherState {
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(DEBOUNCE, tx).expect("failed to create file watcher");

    let watched = {
        let guard = conn.lock().expect("db lock poisoned");
        db::list_watched_folders(&guard).unwrap_or_default()
    };
    for folder in &watched {
        let _ = debouncer
            .watcher()
            .watch(Path::new(folder), RecursiveMode::Recursive);
    }

    std::thread::spawn(move || {
        for result in rx {
            let Ok(events) = result else { continue };
            if events.is_empty() {
                continue;
            }

            let folders = {
                let Ok(guard) = conn.lock() else { continue };
                db::list_watched_folders(&guard).unwrap_or_default()
            };

            for folder in folders {
                let discovered = indexer::discover_files(&folder);
                if let Ok(guard) = conn.lock() {
                    let _ = indexer::insert_batch(&guard, &discovered);
                }
            }

            let _ = app_handle.emit(SAMPLES_UPDATED_EVENT, ());
        }
    });

    WatcherState(Mutex::new(debouncer))
}
