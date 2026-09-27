mod analysis;
mod db;
mod indexer;
mod metadata;
mod samples;
mod watcher;

use db::DbState;
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = Arc::new(Mutex::new(db::init(app.handle())));
            app.manage(DbState(conn.clone()));
            app.manage(watcher::init(app.handle().clone(), conn));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            indexer::scan_directory,
            samples::list_samples,
            samples::list_virtual_tags,
            samples::create_tag,
            samples::assign_tag,
            samples::remove_tag,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
