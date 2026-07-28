mod db;
mod indexer;
mod metadata;
mod samples;

use db::DbState;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = db::init(app.handle());
            app.manage(DbState(Mutex::new(conn)));
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
