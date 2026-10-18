// SPDX-License-Identifier: GPL-3.0-only
//! Tauri shell. Phase 0 only proves the UI <-> Rust <-> database path.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use stillwave_db::Db;
use tauri::{Manager, State};

/// Shared handle to the open library database.
struct AppDb(Arc<Mutex<Db>>);

/// What the placeholder home screen shows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    app_name: &'static str,
    app_version: &'static str,
    core_version: &'static str,
    schema_version: u32,
    db_path: String,
}

/// Reads on a blocking thread so the UI thread never waits on `SQLite`.
#[tauri::command]
async fn app_info(db: State<'_, AppDb>) -> Result<AppInfo, String> {
    let db = Arc::clone(&db.0);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        Ok(AppInfo {
            app_name: "Stillwave",
            app_version: env!("CARGO_PKG_VERSION"),
            core_version: stillwave_core::VERSION,
            schema_version: db.schema_version().map_err(|e| e.to_string())?,
            db_path: db
                .path()
                .map_or_else(|| "(in memory)".into(), |p| p.display().to_string()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts the app.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start; there is nothing useful to do then.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(dir.join("stillwave.db"))?;
            app.manage(AppDb(Arc::new(Mutex::new(db))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running the Stillwave app");
}
