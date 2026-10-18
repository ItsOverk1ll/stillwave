// SPDX-License-Identifier: GPL-3.0-only
//! Tauri shell: scanning, library browsing and playback for the UI.
//!
//! Every command that touches the database or the disk runs on a blocking
//! thread, and the database lock is held only for one query or one import batch
//! at a time, so the UI stays responsive during a scan.

mod browse;
mod playback;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use stillwave_audio::Player;
use stillwave_db::{Db, DbError, ImportCounts, StoredFile, TrackImport};
use stillwave_library::{scan, LibraryStore, ScanOptions, ScanProgress};
use tauri::{AppHandle, Emitter, Manager, State};

use playback::{AudioDevice, AudioMode, PlayerView, QueueEntry};

/// Most tracks queued when the user presses play on a row.
const MAX_QUEUE: u32 = 1000;
/// Most tracks queued when playing an artist.
const MAX_ARTIST_QUEUE: u32 = 2000;

type SharedDb = Arc<Mutex<Db>>;

pub(crate) struct AppState {
    db: SharedDb,
    scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    playback: Mutex<Playback>,
}

#[derive(Default)]
struct Playback {
    player: Option<Player>,
    /// What the UI shows for each queue entry, and the file each one plays.
    queue: Vec<QueueEntry>,
    paths: Vec<PathBuf>,
    /// Bumped every time the queue is replaced.
    version: u64,
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    m.lock().map_err(|_| "internal lock poisoned".to_string())
}

/// Lets the scanner use the shared database, locking per call.
struct SharedStore(SharedDb);

impl SharedStore {
    fn db(&self) -> Result<MutexGuard<'_, Db>, DbError> {
        self.0
            .lock()
            .map_err(|_| DbError::Corrupt("database lock poisoned".into()))
    }
}

impl LibraryStore for SharedStore {
    fn stored_files(&mut self) -> Result<HashMap<String, StoredFile>, DbError> {
        self.db()?.stored_files()
    }
    fn import_tracks(&mut self, batch: &[TrackImport]) -> Result<ImportCounts, DbError> {
        self.db()?.import_tracks(batch)
    }
    fn remove_files(&mut self, paths: &[String]) -> Result<u32, DbError> {
        self.db()?.remove_files(paths)
    }
}

// ---- app info ---------------------------------------------------------------

/// What the home screen shows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    app_name: &'static str,
    app_version: &'static str,
    core_version: &'static str,
    schema_version: u32,
    db_path: String,
}

/// Runs `f` with the database on a blocking thread.
pub(crate) async fn with_db<T: Send + 'static>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&Db) -> Result<T, DbError> + Send + 'static,
) -> Result<T, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = lock(&db)?;
        f(&db).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn app_info(state: State<'_, AppState>) -> Result<AppInfo, String> {
    with_db(&state, |db| {
        Ok(AppInfo {
            app_name: "Stillwave",
            app_version: env!("CARGO_PKG_VERSION"),
            core_version: stillwave_core::VERSION,
            schema_version: db.schema_version()?,
            db_path: db
                .path()
                .map_or_else(|| "(in memory)".into(), |p| p.display().to_string()),
        })
    })
    .await
}

// ---- settings ---------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    music_folder: Option<String>,
    /// `exclusive`, `exclusive_fallback` or `shared`.
    audio_mode: &'static str,
    audio_device_id: Option<String>,
}

fn read_settings(db: &Db) -> Result<Settings, DbError> {
    Ok(Settings {
        music_folder: db.setting("music_folder")?,
        audio_mode: db
            .setting("audio_mode")?
            .as_deref()
            .and_then(AudioMode::parse)
            .unwrap_or(AudioMode::Exclusive)
            .as_str(),
        audio_device_id: db.setting("audio_device_id")?,
    })
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    with_db(&state, read_settings).await
}

#[tauri::command]
async fn set_music_folder(state: State<'_, AppState>, path: String) -> Result<(), String> {
    if !PathBuf::from(&path).is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    with_db(&state, move |db| db.set_setting("music_folder", &path)).await
}

#[tauri::command]
async fn list_audio_devices() -> Vec<AudioDevice> {
    tauri::async_runtime::spawn_blocking(playback::list_devices)
        .await
        .unwrap_or_default()
}

/// Saves how audio is output and drops the current player so the next play uses it.
#[tauri::command]
async fn set_audio_options(
    state: State<'_, AppState>,
    mode: String,
    device_id: Option<String>,
) -> Result<(), String> {
    let mode = AudioMode::parse(&mode).ok_or("unknown audio mode")?;
    let device = device_id.filter(|d| !d.is_empty());
    {
        let mut pb = lock(&state.playback)?;
        pb.player = None; // stops playback; rebuilt on the next play
        pb.queue.clear();
    }
    with_db(&state, move |db| {
        db.set_setting("audio_mode", mode.as_str())?;
        match &device {
            Some(d) => db.set_setting("audio_device_id", d),
            None => db.clear_setting("audio_device_id"),
        }
    })
    .await
}

// ---- scanning ---------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanProgressEvent {
    /// `discovering` or `importing`.
    phase: &'static str,
    found: usize,
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanFinishedEvent {
    found: usize,
    added: u32,
    updated: u32,
    unchanged: usize,
    removed: u32,
    /// First few unreadable files, as `path: reason`.
    failed: Vec<String>,
    failed_count: usize,
    cancelled: bool,
    error: Option<String>,
}

/// Starts scanning in the background. Progress arrives as `scan-progress` events
/// and the result as one `scan-finished` event.
#[tauri::command]
async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = lock(&state.scan_cancel)?;
        if slot.is_some() {
            return Err("A scan is already running.".into());
        }
        *slot = Some(Arc::clone(&cancel));
    }
    let db = Arc::clone(&state.db);
    let remember = path.clone();
    std::thread::Builder::new()
        .name("stillwave-scan".into())
        .spawn(move || {
            let mut store = SharedStore(Arc::clone(&db));
            let options = ScanOptions {
                cancel,
                force: false,
                remove_missing: true,
            };
            let mut found = 0;
            let result = scan(&mut store, &root, &options, |p| {
                let ev = match p {
                    ScanProgress::Discovering { found: f } => {
                        found = f;
                        ScanProgressEvent {
                            phase: "discovering",
                            found: f,
                            done: 0,
                            total: 0,
                        }
                    }
                    ScanProgress::Importing { done, total } => ScanProgressEvent {
                        phase: "importing",
                        found,
                        done,
                        total,
                    },
                };
                let _ = app.emit("scan-progress", ev);
            });
            let finished = match result {
                Ok(r) => {
                    if let Ok(db) = db.lock() {
                        let _ = db.set_setting("music_folder", &remember);
                        // New rows change the best query plans; keep statistics fresh.
                        let _ = db.optimize();
                    }
                    ScanFinishedEvent {
                        found: r.found,
                        added: r.added,
                        updated: r.updated,
                        unchanged: r.unchanged,
                        removed: r.removed,
                        failed_count: r.failed.len(),
                        failed: r
                            .failed
                            .iter()
                            .take(10)
                            .map(|(p, e)| format!("{}: {e}", p.display()))
                            .collect(),
                        cancelled: r.cancelled,
                        error: None,
                    }
                }
                Err(e) => ScanFinishedEvent {
                    found: 0,
                    added: 0,
                    updated: 0,
                    unchanged: 0,
                    removed: 0,
                    failed: Vec::new(),
                    failed_count: 0,
                    cancelled: false,
                    error: Some(e.to_string()),
                },
            };
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut slot) = state.scan_cancel.lock() {
                    *slot = None;
                }
            }
            let _ = app.emit("scan-finished", finished);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(c) = lock(&state.scan_cancel)?.as_ref() {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ---- playback ---------------------------------------------------------------

/// Where a play started from; decides what goes in the queue.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PlayContext {
    /// The songs list, as filtered by `query`. The queue runs from the chosen row
    /// to the end of the list (capped).
    Library { query: String },
    /// A whole album, in track order.
    Album { id: String },
    /// Every track by an artist, in library order.
    Artist { id: String },
}

/// Plays `context` starting at its row `index`.
#[tauri::command]
async fn play(state: State<'_, AppState>, context: PlayContext, index: u32) -> Result<(), String> {
    let (settings, rows, start) = with_db(&state, move |db| {
        let settings = read_settings(db)?;
        Ok(match context {
            PlayContext::Library { query } => {
                (settings, db.library_rows(&query, MAX_QUEUE, index)?, 0)
            }
            PlayContext::Album { id } => (settings, db.album_tracks(&id)?, index),
            PlayContext::Artist { id } => {
                (settings, db.artist_tracks(&id, MAX_ARTIST_QUEUE)?, index)
            }
        })
    })
    .await?;

    // Missing files cannot play. Keep the chosen track's position correct when
    // some rows before it are dropped.
    let chosen = rows.get(start as usize).map(|r| r.item_id.clone());
    let playable: Vec<_> = rows.into_iter().filter(|r| r.path.is_some()).collect();
    if playable.is_empty() {
        return Err("Nothing playable here: the files are missing from this device.".into());
    }
    let start = chosen
        .and_then(|id| playable.iter().position(|r| r.item_id == id))
        .unwrap_or(0);
    let queue: Vec<QueueEntry> = playable
        .iter()
        .map(|r| QueueEntry {
            item_id: r.item_id.clone(),
            title: r.title.clone(),
            artist: r.artist.clone(),
            album: r.album.clone(),
            duration_ms: r.duration_ms,
        })
        .collect();
    let paths: Vec<PathBuf> = playable
        .iter()
        .filter_map(|r| r.path.clone())
        .map(PathBuf::from)
        .collect();

    let mode = AudioMode::parse(settings.audio_mode).unwrap_or(AudioMode::Exclusive);
    let mut pb = lock(&state.playback)?;
    if pb.player.is_none() {
        pb.player = Some(playback::make_player(
            mode,
            settings.audio_device_id.as_deref(),
        )?);
    }
    pb.queue = queue;
    pb.paths.clone_from(&paths);
    pb.version += 1;
    if let Some(p) = &pb.player {
        p.play_at(paths, start);
    }
    Ok(())
}

#[derive(Serialize)]
struct QueueView {
    /// Changes whenever the queue is replaced; the UI refetches when it does.
    version: u64,
    entries: Vec<QueueEntry>,
}

#[tauri::command]
fn play_queue(state: State<'_, AppState>) -> Result<QueueView, String> {
    let pb = lock(&state.playback)?;
    Ok(QueueView {
        version: pb.version,
        entries: pb.queue.clone(),
    })
}

/// Jumps to entry `index` of the current queue.
#[tauri::command]
fn jump_to(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    let pb = lock(&state.playback)?;
    if index >= pb.paths.len() {
        return Err("That queue entry no longer exists.".into());
    }
    if let Some(p) = &pb.player {
        p.play_at(pb.paths.clone(), index);
    }
    Ok(())
}

fn with_player(state: &State<'_, AppState>, f: impl FnOnce(&Player)) -> Result<(), String> {
    if let Some(p) = &lock(&state.playback)?.player {
        f(p);
    }
    Ok(())
}

#[tauri::command]
fn pause(state: State<'_, AppState>) -> Result<(), String> {
    with_player(&state, Player::pause)
}

#[tauri::command]
fn resume(state: State<'_, AppState>) -> Result<(), String> {
    with_player(&state, Player::resume)
}

#[tauri::command]
fn stop(state: State<'_, AppState>) -> Result<(), String> {
    with_player(&state, Player::stop)
}

#[tauri::command]
fn next_track(state: State<'_, AppState>) -> Result<(), String> {
    with_player(&state, Player::next)
}

/// Seeks within the current track to `position_ms`.
#[tauri::command]
fn seek(state: State<'_, AppState>, position_ms: u64) -> Result<(), String> {
    let pb = lock(&state.playback)?;
    if let Some(p) = &pb.player {
        let rate = p.status().format.map_or(0, |f| u64::from(f.sample_rate_hz));
        p.seek(position_ms * rate / 1000);
    }
    Ok(())
}

#[tauri::command]
fn player_status(state: State<'_, AppState>) -> Result<PlayerView, String> {
    let pb = lock(&state.playback)?;
    Ok(pb.player.as_ref().map_or_else(PlayerView::idle, |p| {
        PlayerView::from_status(&p.status(), &pb.queue, pb.version)
    }))
}

/// Starts the app.
///
/// # Panics
///
/// Panics if the Tauri runtime cannot start; there is nothing useful to do then.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(dir.join("stillwave.db"))?;
            app.manage(AppState {
                db: Arc::new(Mutex::new(db)),
                scan_cancel: Mutex::new(None),
                playback: Mutex::new(Playback::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_settings,
            set_music_folder,
            list_audio_devices,
            set_audio_options,
            start_scan,
            cancel_scan,
            browse::library_page,
            browse::albums_page,
            browse::album_detail,
            browse::artists_page,
            browse::artist_detail,
            play,
            play_queue,
            jump_to,
            pause,
            resume,
            stop,
            next_track,
            seek,
            player_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Stillwave app");
}
