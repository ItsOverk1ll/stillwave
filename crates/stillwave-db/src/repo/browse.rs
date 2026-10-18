// SPDX-License-Identifier: GPL-3.0-only
//! Read paths for the library screen (paged, optionally filtered by search) and
//! small local settings. Settings are per-device preferences, not user data, so
//! they are not written to the change log.

use rusqlite::{params, OptionalExtension};

use super::{fts_query, meta, set_meta, Db, Result};

/// One row of the library list: an item with its names and file facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRow {
    pub item_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub duration_ms: Option<u64>,
    /// File on this device, if the item has one (a vanished file leaves none).
    pub path: Option<String>,
    pub codec: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
}

/// Joins are applied only to the rows of one page: `page` is a subquery that picks
/// the page's row IDs from an index first. Joining before paging would run every
/// join for every skipped row, which takes over a second at the end of 50,000 rows.
const ROW_COLUMNS: &str = "SELECT m.id, m.title, a.name, al.title, m.track_number, m.duration_ms,
        f.path, f.codec, f.sample_rate_hz, f.bit_depth, f.channels";

const ROW_JOINS: &str = "JOIN media_items m ON m.rowid = page.rid
    LEFT JOIN artists a ON a.id = m.artist_id
    LEFT JOIN albums al ON al.id = m.album_id
    LEFT JOIN media_files f ON f.media_item_id = m.id AND f.device_id = ?1";

impl Db {
    /// Rows for the library screen. `query` filters by search (title, artist, album,
    /// prefix match); without it the list is in library order. `limit` is capped at 500.
    pub fn library_rows(&self, query: &str, limit: u32, offset: u32) -> Result<Vec<LibraryRow>> {
        let limit = limit.min(500);
        let device = self.device.as_bytes().as_slice();
        let map = |r: &rusqlite::Row| -> rusqlite::Result<(Vec<u8>, LibraryRow)> {
            let id: Vec<u8> = r.get(0)?;
            Ok((
                id,
                LibraryRow {
                    item_id: String::new(),
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    track_number: r.get(4)?,
                    duration_ms: r.get(5)?,
                    path: r.get(6)?,
                    codec: r.get(7)?,
                    sample_rate_hz: r.get(8)?,
                    bit_depth: r.get(9)?,
                    channels: r.get(10)?,
                },
            ))
        };
        let rows: Vec<(Vec<u8>, LibraryRow)> = if let Some(fts) = fts_query(query) {
            let mut stmt = self.conn.prepare_cached(&format!(
                "{ROW_COLUMNS} FROM (
                     SELECT rowid AS rid, rank FROM search_index
                     WHERE search_index MATCH ?2 ORDER BY rank LIMIT ?3 OFFSET ?4
                 ) page {ROW_JOINS} ORDER BY page.rank"
            ))?;
            let it = stmt.query_map(params![device, fts, limit, offset], map)?;
            it.collect::<std::result::Result<_, _>>()?
        } else {
            let mut stmt = self.conn.prepare_cached(&format!(
                "{ROW_COLUMNS} FROM (
                     SELECT rowid AS rid, sort_key FROM media_items
                     ORDER BY sort_key, rowid LIMIT ?2 OFFSET ?3
                 ) page {ROW_JOINS} ORDER BY page.sort_key, page.rid"
            ))?;
            let it = stmt.query_map(params![device, limit, offset], map)?;
            it.collect::<std::result::Result<_, _>>()?
        };
        rows.into_iter()
            .map(|(id, mut row)| {
                let id = super::to_id(&id)?;
                row.item_id = stillwave_core::MediaItemId::from_bytes(id).to_string();
                Ok(row)
            })
            .collect()
    }

    /// Number of rows `library_rows` would return in total for `query`.
    pub fn library_count(&self, query: &str) -> Result<u64> {
        if let Some(fts) = fts_query(query) {
            Ok(self.conn.query_row(
                "SELECT COUNT(*) FROM search_index WHERE search_index MATCH ?1",
                params![fts],
                |r| r.get(0),
            )?)
        } else {
            self.media_item_count()
        }
    }

    /// A local preference, or `None` if unset.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        meta(&self.conn, &format!("setting.{key}"))
    }

    /// Stores a local preference.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        set_meta(&self.conn, &format!("setting.{key}"), value)
    }

    /// Removes a local preference.
    pub fn clear_setting(&self, key: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM meta WHERE key = ?1",
            params![format!("setting.{key}")],
        )?;
        Ok(())
    }

    /// File path of an item on this device.
    pub fn item_path(&self, item: stillwave_core::MediaItemId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT path FROM media_files WHERE media_item_id = ?1 AND device_id = ?2 LIMIT 1",
                params![
                    item.as_bytes().as_slice(),
                    self.device.as_bytes().as_slice()
                ],
                |r| r.get(0),
            )
            .optional()?)
    }
}
