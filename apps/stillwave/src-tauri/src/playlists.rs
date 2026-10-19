// SPDX-License-Identifier: GPL-3.0-only
//! Playlist commands: Spotify import, the playlists list, playlist pages and
//! Liked Songs.

use std::path::PathBuf;

use serde::Serialize;
use stillwave_db::{Db, DbError, EntryRow, LibraryRow, PlaylistRow};
use tauri::State;

use crate::browse::TrackRow;
use crate::{lock, AppState};

/// Like `with_db`, for commands that write.
async fn with_db_mut<T: Send + 'static>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut Db) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let db = std::sync::Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let mut db = lock(&db)?;
        f(&mut db)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPlaylist {
    id: String,
    name: String,
    total: u32,
    matched: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    playlists: Vec<ImportedPlaylist>,
    /// Liked songs found in the export: total and how many are in the library.
    liked_total: Option<u32>,
    liked_matched: Option<u32>,
    skipped: u32,
    /// "file: reason" for files that could not be read.
    problems: Vec<String>,
}

/// Imports a Spotify data download (folder) or CSV playlist exports (files).
/// Reads only the chosen files; nothing is sent anywhere.
#[tauri::command]
pub async fn import_spotify(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<ImportResult, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    with_db_mut(&state, move |db| {
        let s = stillwave_library::import_spotify(db, &paths).map_err(|e| e.to_string())?;
        Ok(ImportResult {
            playlists: s
                .playlists
                .into_iter()
                .map(|p| ImportedPlaylist {
                    id: p.id,
                    name: p.name,
                    total: p.total,
                    matched: p.matched,
                })
                .collect(),
            liked_total: s.liked.map(|(t, _)| t),
            liked_matched: s.liked.map(|(_, m)| m),
            skipped: s.skipped,
            problems: s
                .problems
                .into_iter()
                .map(|(f, e)| format!("{f}: {e}"))
                .collect(),
        })
    })
    .await
}

/// After a scan: songs that were missing may now be in the library.
pub fn after_scan(db: &mut Db) {
    let _ = stillwave_library::resolve_placeholders(db);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistView {
    id: String,
    name: String,
    description: Option<String>,
    entry_count: u32,
    owned_count: u32,
}

impl From<PlaylistRow> for PlaylistView {
    fn from(p: PlaylistRow) -> Self {
        Self {
            id: p.id,
            name: p.name,
            description: p.description,
            entry_count: p.entry_count,
            owned_count: p.owned_count,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistsView {
    playlists: Vec<PlaylistView>,
    liked_count: u32,
    liked_owned: u32,
}

#[tauri::command]
pub async fn playlists(state: State<'_, AppState>) -> Result<PlaylistsView, String> {
    crate::with_db(&state, |db| {
        let (liked_count, liked_owned) = db.like_counts()?;
        Ok(PlaylistsView {
            playlists: db
                .playlists()?
                .into_iter()
                .map(PlaylistView::from)
                .collect(),
            liked_count,
            liked_owned,
        })
    })
    .await
}

#[derive(Serialize)]
pub struct PlaylistDetail {
    playlist: PlaylistView,
    entries: Vec<TrackRow>,
}

#[tauri::command]
pub async fn playlist_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<PlaylistDetail, String> {
    crate::with_db(&state, move |db| {
        let playlist = db
            .playlist(&id)?
            .ok_or_else(|| DbError::Corrupt("this playlist is no longer in the library".into()))?;
        Ok(PlaylistDetail {
            playlist: playlist.into(),
            entries: db
                .playlist_entries(&id)?
                .into_iter()
                .map(entry_view)
                .collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn liked_detail(state: State<'_, AppState>) -> Result<Vec<TrackRow>, String> {
    crate::with_db(&state, |db| {
        Ok(db.liked_entries()?.into_iter().map(entry_view).collect())
    })
    .await
}

fn entry_view(e: EntryRow) -> TrackRow {
    match e {
        EntryRow::Track(t) => TrackRow::from(t),
        missing @ EntryRow::Missing { .. } => TrackRow::missing(entry_row(missing)),
    }
}

/// An entry as a library row; songs not owned have no file (so they never play)
/// and carry their placeholder ID.
pub fn entry_row(e: EntryRow) -> LibraryRow {
    match e {
        EntryRow::Track(t) => t,
        EntryRow::Missing {
            placeholder_id,
            title,
            artist,
            album,
            duration_ms,
        } => LibraryRow {
            item_id: placeholder_id,
            title,
            artist: (!artist.is_empty()).then_some(artist),
            artist_id: None,
            album,
            album_id: None,
            track_number: None,
            duration_ms,
            path: None,
            codec: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
        },
    }
}
