// SPDX-License-Identifier: GPL-3.0-only
//! Albums and artists for browsing. Every page first picks the IDs it shows and
//! only then computes counts and quality for those rows, so a page costs the same
//! at the top of the list as at the end.

use rusqlite::{params, OptionalExtension, Row};
use stillwave_core::{AlbumId, ArtistId};

use super::browse::{finish_rows, raw_row, LibraryRow, RawRow, ROW_COLUMNS, ROW_JOINS};
use super::{to_id, Db, DbError, Result};

/// An album with what the browse screens show about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumRow {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub artist_id: Option<String>,
    pub year: Option<i32>,
    pub track_count: u32,
    pub duration_ms: u64,
    /// The codec when every track shares one (e.g. all FLAC), else `None`.
    pub codec: Option<String>,
    /// Highest sample rate and bit depth among the album's files.
    pub max_sample_rate_hz: Option<u32>,
    pub max_bit_depth: Option<u8>,
}

/// An artist with counts for the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistRow {
    pub id: String,
    pub name: String,
    /// Albums credited to this artist (as album artist).
    pub album_count: u32,
    /// Tracks credited to this artist.
    pub track_count: u32,
}

/// An album on an artist's page. `appears_on` marks albums credited to someone
/// else that include tracks by this artist (compilations, features).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistAlbum {
    pub album: AlbumRow,
    pub appears_on: bool,
}

/// Columns for one album; `?1` is this device. `al` and `ar` must be in scope.
///
/// `CROSS JOIN` pins the join order (album's tracks, then their files). Without
/// table statistics SQLite otherwise starts from every file on the device and
/// an album page took 14 s at 50,000 albums instead of 45 ms.
const ALBUM_COLUMNS: &str = "SELECT al.id, al.title, ar.name, al.album_artist_id, al.year,
    (SELECT COUNT(*) FROM media_items m WHERE m.album_id = al.id),
    (SELECT COALESCE(SUM(m.duration_ms), 0) FROM media_items m WHERE m.album_id = al.id),
    (SELECT CASE WHEN COUNT(DISTINCT f.codec) = 1 THEN MAX(f.codec) END
       FROM media_items m CROSS JOIN media_files f ON f.media_item_id = m.id AND f.device_id = ?1
      WHERE m.album_id = al.id),
    (SELECT MAX(f.sample_rate_hz)
       FROM media_items m CROSS JOIN media_files f ON f.media_item_id = m.id AND f.device_id = ?1
      WHERE m.album_id = al.id),
    (SELECT MAX(f.bit_depth)
       FROM media_items m CROSS JOIN media_files f ON f.media_item_id = m.id AND f.device_id = ?1
      WHERE m.album_id = al.id)";

/// Albums that still have at least one track.
const HAS_TRACKS: &str = "EXISTS (SELECT 1 FROM media_items m WHERE m.album_id = al.id)";

/// Album artist, then title (stored in the indexed `sort_key`).
const ALBUM_ORDER: &str = "al.sort_key, al.id";

const ARTIST_HAS_MUSIC: &str = "(EXISTS (SELECT 1 FROM media_items m WHERE m.artist_id = a.id)
    OR EXISTS (SELECT 1 FROM albums al WHERE al.album_artist_id = a.id
               AND EXISTS (SELECT 1 FROM media_items m WHERE m.album_id = al.id)))";

const ARTIST_COLUMNS: &str = "SELECT a.id, a.name,
    (SELECT COUNT(*) FROM albums al WHERE al.album_artist_id = a.id
        AND EXISTS (SELECT 1 FROM media_items m WHERE m.album_id = al.id)),
    (SELECT COUNT(*) FROM media_items m WHERE m.artist_id = a.id)";

fn album_from_row(r: &Row) -> rusqlite::Result<(Vec<u8>, Option<Vec<u8>>, AlbumRow)> {
    Ok((
        r.get(0)?,
        r.get(3)?,
        AlbumRow {
            id: String::new(),
            title: r.get(1)?,
            artist: r.get(2)?,
            artist_id: None,
            year: r.get(4)?,
            track_count: r.get(5)?,
            duration_ms: r.get(6)?,
            codec: r.get(7)?,
            max_sample_rate_hz: r.get(8)?,
            max_bit_depth: r.get(9)?,
        },
    ))
}

fn finish_album((id, artist, mut a): (Vec<u8>, Option<Vec<u8>>, AlbumRow)) -> Result<AlbumRow> {
    a.id = AlbumId::from_bytes(to_id(&id)?).to_string();
    a.artist_id = artist
        .map(|b| to_id(&b).map(|i| ArtistId::from_bytes(i).to_string()))
        .transpose()?;
    Ok(a)
}

fn artist_from_row(r: &Row) -> rusqlite::Result<(Vec<u8>, ArtistRow)> {
    Ok((
        r.get(0)?,
        ArtistRow {
            id: String::new(),
            name: r.get(1)?,
            album_count: r.get(2)?,
            track_count: r.get(3)?,
        },
    ))
}

fn finish_artist((id, mut a): (Vec<u8>, ArtistRow)) -> Result<ArtistRow> {
    a.id = ArtistId::from_bytes(to_id(&id)?).to_string();
    Ok(a)
}

fn parse_album(id: &str) -> Result<AlbumId> {
    id.parse()
        .map_err(|e: stillwave_core::ParseIdError| DbError::Corrupt(e.to_string()))
}

fn parse_artist(id: &str) -> Result<ArtistId> {
    id.parse()
        .map_err(|e: stillwave_core::ParseIdError| DbError::Corrupt(e.to_string()))
}

impl Db {
    /// Albums that have tracks, ordered by album artist then title. `limit` is capped at 500.
    pub fn albums_page(&self, limit: u32, offset: u32) -> Result<Vec<AlbumRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ALBUM_COLUMNS} FROM (
                 SELECT al.id AS aid FROM albums al
                 WHERE {HAS_TRACKS} ORDER BY {ALBUM_ORDER} LIMIT ?2 OFFSET ?3
             ) page
             JOIN albums al ON al.id = page.aid
             LEFT JOIN artists ar ON ar.id = al.album_artist_id
             ORDER BY {ALBUM_ORDER}"
        ))?;
        let rows = stmt.query_map(
            params![self.device.as_bytes().as_slice(), limit.min(500), offset],
            album_from_row,
        )?;
        rows.map(|r| finish_album(r?)).collect()
    }

    /// Number of albums `albums_page` lists.
    pub fn album_count(&self) -> Result<u64> {
        Ok(self.conn.query_row(
            &format!("SELECT COUNT(*) FROM albums al WHERE {HAS_TRACKS}"),
            [],
            |r| r.get(0),
        )?)
    }

    /// One album, or `None` if the ID is unknown.
    pub fn album(&self, id: &str) -> Result<Option<AlbumRow>> {
        let id = parse_album(id)?;
        self.conn
            .query_row(
                &format!(
                    "{ALBUM_COLUMNS} FROM albums al LEFT JOIN artists ar ON ar.id = al.album_artist_id
                     WHERE al.id = ?2"
                ),
                params![self.device.as_bytes().as_slice(), id.as_bytes().as_slice()],
                album_from_row,
            )
            .optional()?
            .map(finish_album)
            .transpose()
    }

    /// An album's tracks in disc and track order.
    pub fn album_tracks(&self, id: &str) -> Result<Vec<LibraryRow>> {
        let id = parse_album(id)?;
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ROW_COLUMNS} FROM (SELECT rowid AS rid FROM media_items WHERE album_id = ?2) page
             {ROW_JOINS}
             ORDER BY COALESCE(m.disc_number, 0), COALESCE(m.track_number, 0), m.title COLLATE NOCASE"
        ))?;
        let rows: Vec<RawRow> = stmt
            .query_map(
                params![self.device.as_bytes().as_slice(), id.as_bytes().as_slice()],
                raw_row,
            )?
            .collect::<std::result::Result<_, _>>()?;
        finish_rows(rows)
    }

    /// Artists with music, by name. `limit` is capped at 500.
    pub fn artists_page(&self, limit: u32, offset: u32) -> Result<Vec<ArtistRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ARTIST_COLUMNS} FROM artists a WHERE {ARTIST_HAS_MUSIC}
             ORDER BY a.name COLLATE NOCASE, a.id LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map(params![limit.min(500), offset], artist_from_row)?;
        rows.map(|r| finish_artist(r?)).collect()
    }

    /// Number of artists `artists_page` lists.
    pub fn artist_count(&self) -> Result<u64> {
        Ok(self.conn.query_row(
            &format!("SELECT COUNT(*) FROM artists a WHERE {ARTIST_HAS_MUSIC}"),
            [],
            |r| r.get(0),
        )?)
    }

    /// One artist, or `None` if the ID is unknown.
    pub fn artist(&self, id: &str) -> Result<Option<ArtistRow>> {
        let id = parse_artist(id)?;
        self.conn
            .query_row(
                &format!("{ARTIST_COLUMNS} FROM artists a WHERE a.id = ?1"),
                params![id.as_bytes().as_slice()],
                artist_from_row,
            )
            .optional()?
            .map(finish_artist)
            .transpose()
    }

    /// The artist's own albums (oldest first), then albums by others they appear on.
    pub fn artist_albums(&self, id: &str) -> Result<Vec<ArtistAlbum>> {
        let id = parse_artist(id)?;
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ALBUM_COLUMNS}, page.appears_on FROM (
                 SELECT al.id AS aid, 0 AS appears_on FROM albums al
                  WHERE al.album_artist_id = ?2 AND {HAS_TRACKS}
                 UNION
                 SELECT DISTINCT m.album_id, 1 FROM media_items m JOIN albums al ON al.id = m.album_id
                  WHERE m.artist_id = ?2 AND al.album_artist_id IS NOT ?2
             ) page
             JOIN albums al ON al.id = page.aid
             LEFT JOIN artists ar ON ar.id = al.album_artist_id
             ORDER BY page.appears_on, COALESCE(al.year, 9999), al.title COLLATE NOCASE"
        ))?;
        let rows = stmt.query_map(
            params![self.device.as_bytes().as_slice(), id.as_bytes().as_slice()],
            |r| Ok((album_from_row(r)?, r.get::<_, i64>(10)? != 0)),
        )?;
        rows.map(|r| {
            let (raw, appears_on) = r?;
            Ok(ArtistAlbum {
                album: finish_album(raw)?,
                appears_on,
            })
        })
        .collect()
    }

    /// Every track credited to the artist, in library order. `limit` is capped at 2,000.
    pub fn artist_tracks(&self, id: &str, limit: u32) -> Result<Vec<LibraryRow>> {
        let id = parse_artist(id)?;
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ROW_COLUMNS} FROM (
                 SELECT rowid AS rid, sort_key FROM media_items WHERE artist_id = ?2
                 ORDER BY sort_key, rowid LIMIT ?3
             ) page {ROW_JOINS} ORDER BY page.sort_key, page.rid"
        ))?;
        let rows: Vec<RawRow> = stmt
            .query_map(
                params![
                    self.device.as_bytes().as_slice(),
                    id.as_bytes().as_slice(),
                    limit.min(2000)
                ],
                raw_row,
            )?
            .collect::<std::result::Result<_, _>>()?;
        finish_rows(rows)
    }
}
