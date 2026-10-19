// SPDX-License-Identifier: GPL-3.0-only
//! Library browsing commands: songs, albums and artists.

use serde::Serialize;
use stillwave_db::{AlbumRow, ArtistAlbum, ArtistRow, LibraryRow};
use tauri::State;

use crate::{with_db, AppState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    id: String,
    title: String,
    artist: Option<String>,
    artist_id: Option<String>,
    album: Option<String>,
    album_id: Option<String>,
    track_number: Option<u32>,
    duration_ms: Option<u64>,
    codec: Option<String>,
    sample_rate_hz: Option<u32>,
    bit_depth: Option<u8>,
    /// False when the file has gone missing from this device.
    playable: bool,
    /// False for a song in a playlist or Liked Songs that is not in the library.
    in_library: bool,
}

impl TrackRow {
    /// A song that is not in the library (a placeholder from an import).
    pub fn missing(r: LibraryRow) -> Self {
        Self {
            in_library: false,
            ..Self::from(r)
        }
    }
}

impl From<LibraryRow> for TrackRow {
    fn from(r: LibraryRow) -> Self {
        Self {
            playable: r.path.is_some(),
            in_library: true,
            id: r.item_id,
            title: r.title,
            artist: r.artist,
            artist_id: r.artist_id,
            album: r.album,
            album_id: r.album_id,
            track_number: r.track_number,
            duration_ms: r.duration_ms,
            codec: r.codec,
            sample_rate_hz: r.sample_rate_hz,
            bit_depth: r.bit_depth,
        }
    }
}

#[derive(Serialize)]
pub struct Page<T> {
    rows: Vec<T>,
    total: u64,
}

/// One page of songs, filtered by search `query` (empty = everything).
#[tauri::command]
pub async fn library_page(
    state: State<'_, AppState>,
    query: String,
    offset: u32,
    limit: u32,
) -> Result<Page<TrackRow>, String> {
    with_db(&state, move |db| {
        Ok(Page {
            total: db.library_count(&query)?,
            rows: db
                .library_rows(&query, limit, offset)?
                .into_iter()
                .map(TrackRow::from)
                .collect(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumView {
    id: String,
    title: String,
    artist: Option<String>,
    artist_id: Option<String>,
    year: Option<i32>,
    track_count: u32,
    duration_ms: u64,
    codec: Option<String>,
    max_sample_rate_hz: Option<u32>,
    max_bit_depth: Option<u8>,
}

impl From<AlbumRow> for AlbumView {
    fn from(a: AlbumRow) -> Self {
        Self {
            id: a.id,
            title: a.title,
            artist: a.artist,
            artist_id: a.artist_id,
            year: a.year,
            track_count: a.track_count,
            duration_ms: a.duration_ms,
            codec: a.codec,
            max_sample_rate_hz: a.max_sample_rate_hz,
            max_bit_depth: a.max_bit_depth,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistView {
    id: String,
    name: String,
    album_count: u32,
    track_count: u32,
}

impl From<ArtistRow> for ArtistView {
    fn from(a: ArtistRow) -> Self {
        Self {
            id: a.id,
            name: a.name,
            album_count: a.album_count,
            track_count: a.track_count,
        }
    }
}

#[tauri::command]
pub async fn albums_page(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<Page<AlbumView>, String> {
    with_db(&state, move |db| {
        Ok(Page {
            total: db.album_count()?,
            rows: db
                .albums_page(limit, offset)?
                .into_iter()
                .map(AlbumView::from)
                .collect(),
        })
    })
    .await
}

#[derive(Serialize)]
pub struct AlbumDetail {
    album: AlbumView,
    tracks: Vec<TrackRow>,
}

#[tauri::command]
pub async fn album_detail(state: State<'_, AppState>, id: String) -> Result<AlbumDetail, String> {
    with_db(&state, move |db| {
        let album = db.album(&id)?.ok_or_else(|| {
            stillwave_db::DbError::Corrupt("this album is no longer in the library".into())
        })?;
        Ok(AlbumDetail {
            album: album.into(),
            tracks: db
                .album_tracks(&id)?
                .into_iter()
                .map(TrackRow::from)
                .collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn artists_page(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<Page<ArtistView>, String> {
    with_db(&state, move |db| {
        Ok(Page {
            total: db.artist_count()?,
            rows: db
                .artists_page(limit, offset)?
                .into_iter()
                .map(ArtistView::from)
                .collect(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistAlbumView {
    #[serde(flatten)]
    album: AlbumView,
    appears_on: bool,
}

impl From<ArtistAlbum> for ArtistAlbumView {
    fn from(a: ArtistAlbum) -> Self {
        Self {
            album: a.album.into(),
            appears_on: a.appears_on,
        }
    }
}

#[derive(Serialize)]
pub struct ArtistDetail {
    artist: ArtistView,
    albums: Vec<ArtistAlbumView>,
}

#[tauri::command]
pub async fn artist_detail(state: State<'_, AppState>, id: String) -> Result<ArtistDetail, String> {
    with_db(&state, move |db| {
        let artist = db.artist(&id)?.ok_or_else(|| {
            stillwave_db::DbError::Corrupt("this artist is no longer in the library".into())
        })?;
        Ok(ArtistDetail {
            artist: artist.into(),
            albums: db
                .artist_albums(&id)?
                .into_iter()
                .map(ArtistAlbumView::from)
                .collect(),
        })
    })
    .await
}
