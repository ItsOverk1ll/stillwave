// SPDX-License-Identifier: GPL-3.0-only
//! The repository API: open, migrate, write (with change log), read, search.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde_json::json;
use stillwave_core::{
    Album, AlbumId, Artist, ArtistId, DeviceId, EntryTarget, ExternalIds, Hlc, HlcClock, Like,
    LikeId, MediaItem, MediaItemId, MediaKind, Placeholder, PlaceholderId, Play, Playlist,
    PlaylistEntry, PlaylistEntryId, PlaylistId,
};

use crate::error::DbError;
use crate::migrations;

type Result<T> = std::result::Result<T, DbError>;

/// An open Stillwave database.
pub struct Db {
    conn: Connection,
    clock: HlcClock,
    device: DeviceId,
    path: Option<PathBuf>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` and migrates it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn, Some(path.to_path_buf()))
    }

    /// Opens a throw-away in-memory database (tests, benchmarks).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        migrations::migrate(&mut conn)?;

        let device = if let Some(s) = meta(&conn, "device_id")? {
            s.parse::<DeviceId>()
                .map_err(|e| DbError::Corrupt(e.to_string()))?
        } else {
            let id = DeviceId::new();
            set_meta(&conn, "device_id", &id.to_string())?;
            id
        };
        let clock = match meta(&conn, "last_hlc")? {
            Some(s) => HlcClock::resume(device, serde_json::from_str::<Hlc>(&s)?),
            None => HlcClock::new(device),
        };
        Ok(Self {
            conn,
            clock,
            device,
            path,
        })
    }

    /// This database's device ID.
    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        self.device
    }

    /// File path, or `None` for in-memory databases.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Schema version of the open database.
    pub fn schema_version(&self) -> Result<u32> {
        migrations::current_version(&self.conn)
    }

    /// Runs `SQLite`'s integrity check; `true` when the file is healthy.
    pub fn integrity_ok(&self) -> Result<bool> {
        let r: String = self
            .conn
            .pragma_query_value(None, "integrity_check", |r| r.get(0))?;
        Ok(r == "ok")
    }

    /// Number of rows in the change log.
    pub fn change_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM change_log", [], |r| r.get(0))?)
    }

    /// Runs `f` in one transaction, then persists the clock and commits. If `f`
    /// fails, nothing (data or change log) is written.
    pub(crate) fn write<T>(
        &mut self,
        f: impl FnOnce(&Transaction, &mut Logger) -> Result<T>,
    ) -> Result<T> {
        let tx = self.conn.transaction()?;
        let mut logger = Logger {
            clock: &mut self.clock,
            device: self.device,
        };
        let out = f(&tx, &mut logger)?;
        let last = serde_json::to_string(&logger.clock.last())?;
        set_meta(&tx, "last_hlc", &last)?;
        tx.commit()?;
        Ok(out)
    }

    // ---- writes -----------------------------------------------------------

    pub fn insert_artist(&mut self, a: &Artist) -> Result<()> {
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO artists (id, name, sort_name, external_ids) VALUES (?1, ?2, ?3, ?4)",
                params![
                    a.id.as_bytes().as_slice(),
                    a.name,
                    a.sort_name,
                    serde_json::to_string(&a.external_ids)?
                ],
            )?;
            log.record(
                tx,
                "artist",
                a.id.as_bytes(),
                "insert",
                &["name", "sort_name", "external_ids"],
            )
        })
    }

    pub fn insert_album(&mut self, a: &Album) -> Result<()> {
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO albums (id, title, album_artist_id, year, external_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    a.id.as_bytes().as_slice(),
                    a.title,
                    a.album_artist_id.map(|i| i.as_bytes().to_vec()),
                    a.year,
                    serde_json::to_string(&a.external_ids)?
                ],
            )?;
            log.record(
                tx,
                "album",
                a.id.as_bytes(),
                "insert",
                &["title", "album_artist_id", "year", "external_ids"],
            )
        })
    }

    pub fn insert_media_item(&mut self, item: &MediaItem) -> Result<()> {
        self.insert_media_items(std::slice::from_ref(item))
    }

    /// Inserts many items in a single transaction (used by scans and imports).
    pub fn insert_media_items(&mut self, items: &[MediaItem]) -> Result<()> {
        self.write(|tx, log| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO media_items (id, kind, title, artist_id, album_id, track_number,
                                          disc_number, duration_ms, isrc, external_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for i in items {
                stmt.execute(params![
                    i.id.as_bytes().as_slice(),
                    i.kind.as_str(),
                    i.title,
                    i.artist_id.map(|v| v.as_bytes().to_vec()),
                    i.album_id.map(|v| v.as_bytes().to_vec()),
                    i.track_number,
                    i.disc_number,
                    i.duration_ms,
                    i.isrc,
                    serde_json::to_string(&i.external_ids)?
                ])?;
                log.record(
                    tx,
                    "media_item",
                    i.id.as_bytes(),
                    "insert",
                    &[
                        "kind",
                        "title",
                        "artist_id",
                        "album_id",
                        "track_number",
                        "disc_number",
                        "duration_ms",
                        "isrc",
                        "external_ids",
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn insert_placeholder(&mut self, p: &Placeholder) -> Result<()> {
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO placeholders (id, title, artist, album, duration_ms, isrc, external_ids, resolved_to)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    p.id.as_bytes().as_slice(),
                    p.title,
                    p.artist,
                    p.album,
                    p.duration_ms,
                    p.isrc,
                    serde_json::to_string(&p.external_ids)?,
                    p.resolved_to.map(|v| v.as_bytes().to_vec())
                ],
            )?;
            log.record(tx, "placeholder", p.id.as_bytes(), "insert", &["title", "artist", "album", "duration_ms", "isrc", "external_ids", "resolved_to"])
        })
    }

    pub fn insert_playlist(&mut self, p: &Playlist) -> Result<()> {
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO playlists (id, name, description) VALUES (?1, ?2, ?3)",
                params![p.id.as_bytes().as_slice(), p.name, p.description],
            )?;
            log.record(
                tx,
                "playlist",
                p.id.as_bytes(),
                "insert",
                &["name", "description"],
            )
        })
    }

    pub fn insert_playlist_entry(&mut self, e: &PlaylistEntry) -> Result<()> {
        let (item, ph) = split_target(e.target);
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO playlist_entries (id, playlist_id, media_item_id, placeholder_id, position)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    e.id.as_bytes().as_slice(),
                    e.playlist_id.as_bytes().as_slice(),
                    item,
                    ph,
                    e.position
                ],
            )?;
            log.record(tx, "playlist_entry", e.id.as_bytes(), "insert", &["playlist_id", "target", "position"])
        })
    }

    pub fn insert_like(&mut self, l: &Like) -> Result<()> {
        let (item, ph) = split_target(l.target);
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO likes (id, media_item_id, placeholder_id, liked_at) VALUES (?1, ?2, ?3, ?4)",
                params![l.id.as_bytes().as_slice(), item, ph, l.liked_at.to_string()],
            )?;
            log.record(tx, "like", l.id.as_bytes(), "insert", &["target", "liked_at"])
        })
    }

    pub fn insert_play(&mut self, p: &Play) -> Result<()> {
        self.write(|tx, log| {
            tx.execute(
                "INSERT INTO plays (id, media_item_id, device_id, started_unix_ms, played_ms, completed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    p.id.as_bytes().as_slice(),
                    p.media_item_id.as_bytes().as_slice(),
                    p.device_id.as_bytes().as_slice(),
                    p.started_unix_ms,
                    p.played_ms,
                    p.completed
                ],
            )?;
            log.record(tx, "play", p.id.as_bytes(), "insert", &["media_item_id", "device_id", "started_unix_ms", "played_ms", "completed"])
        })
    }

    /// Renames a media item (also refreshes its search entry via triggers).
    pub fn rename_media_item(&mut self, id: MediaItemId, title: &str) -> Result<()> {
        self.write(|tx, log| {
            let n = tx.execute(
                "UPDATE media_items SET title = ?1 WHERE id = ?2",
                params![title, id.as_bytes().as_slice()],
            )?;
            if n > 0 {
                log.record(tx, "media_item", id.as_bytes(), "update", &["title"])?;
            }
            Ok(())
        })
    }

    pub fn delete_media_item(&mut self, id: MediaItemId) -> Result<()> {
        self.write(|tx, log| {
            let n = tx.execute(
                "DELETE FROM media_items WHERE id = ?1",
                params![id.as_bytes().as_slice()],
            )?;
            if n > 0 {
                log.record(tx, "media_item", id.as_bytes(), "delete", &[])?;
            }
            Ok(())
        })
    }

    // ---- reads ------------------------------------------------------------

    pub fn get_media_item(&self, id: MediaItemId) -> Result<Option<MediaItem>> {
        self.conn
            .query_row(
                &format!("{ITEM_SELECT} WHERE m.id = ?1"),
                params![id.as_bytes().as_slice()],
                item_from_row,
            )
            .optional()?
            .transpose()
    }

    pub fn list_media_items(&self, limit: u32, offset: u32) -> Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ITEM_SELECT} ORDER BY m.rowid LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map(params![limit, offset], item_from_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn media_item_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM media_items", [], |r| r.get(0))?)
    }

    pub fn get_artist(&self, id: ArtistId) -> Result<Option<Artist>> {
        self.conn
            .query_row(
                "SELECT id, name, sort_name, external_ids FROM artists WHERE id = ?1",
                params![id.as_bytes().as_slice()],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(id, name, sort_name, ext)| {
                Ok(Artist {
                    id: ArtistId::from_bytes(to_id(&id)?),
                    name,
                    sort_name,
                    external_ids: parse_ext(&ext)?,
                })
            })
            .transpose()
    }

    pub fn list_playlist_entries(&self, playlist: PlaylistId) -> Result<Vec<PlaylistEntry>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, media_item_id, placeholder_id, position FROM playlist_entries
             WHERE playlist_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![playlist.as_bytes().as_slice()], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Option<Vec<u8>>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|r| {
            let (id, item, ph, position) = r?;
            let target = match (item, ph) {
                (Some(i), None) => EntryTarget::Item(MediaItemId::from_bytes(to_id(&i)?)),
                (None, Some(p)) => EntryTarget::Placeholder(PlaceholderId::from_bytes(to_id(&p)?)),
                _ => return Err(DbError::Corrupt("playlist entry target".into())),
            };
            Ok(PlaylistEntry {
                id: PlaylistEntryId::from_bytes(to_id(&id)?),
                playlist_id: playlist,
                target,
                position,
            })
        })
        .collect()
    }

    pub fn is_liked(&self, target: EntryTarget) -> Result<bool> {
        let (item, ph) = split_target(target);
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM likes WHERE media_item_id IS ?1 AND placeholder_id IS ?2)",
            params![item, ph],
            |r| r.get(0),
        )?)
    }

    pub fn get_like(&self, id: LikeId) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM likes WHERE id = ?1)",
            params![id.as_bytes().as_slice()],
            |r| r.get(0),
        )?)
    }

    /// Search-as-you-type over title, artist and album. Every word is a prefix
    /// match and all words must match. Blank queries return nothing.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<MediaItem>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare_cached(&format!(
            "{ITEM_SELECT} JOIN search_index s ON s.rowid = m.rowid
             WHERE search_index MATCH ?1 ORDER BY s.rank LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![fts, limit], item_from_row)?;
        rows.map(|r| r?).collect()
    }
}

/// Writes change-log rows with fresh clock ticks.
pub(crate) struct Logger<'a> {
    clock: &'a mut HlcClock,
    device: DeviceId,
}

impl Logger<'_> {
    fn record(
        &mut self,
        tx: &Transaction,
        entity: &str,
        id: &[u8; 16],
        op: &str,
        fields: &[&str],
    ) -> Result<()> {
        self.record_raw(tx, entity, id, op, fields)
    }

    fn record_raw(
        &mut self,
        tx: &Transaction,
        entity: &str,
        id: &[u8],
        op: &str,
        fields: &[&str],
    ) -> Result<()> {
        let hlc = self.clock.tick();
        tx.prepare_cached(
            "INSERT INTO change_log (entity_type, entity_id, op, changed_fields, hlc, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![
            entity,
            id,
            op,
            json!(fields).to_string(),
            hlc.to_string(),
            self.device.as_bytes().as_slice()
        ])?;
        Ok(())
    }
}

const ITEM_SELECT: &str = "SELECT m.id, m.kind, m.title, m.artist_id, m.album_id, m.track_number,
        m.disc_number, m.duration_ms, m.isrc, m.external_ids FROM media_items m";

fn split_target(t: EntryTarget) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    match t {
        EntryTarget::Item(i) => (Some(i.as_bytes().to_vec()), None),
        EntryTarget::Placeholder(p) => (None, Some(p.as_bytes().to_vec())),
    }
}

fn to_id(b: &[u8]) -> Result<[u8; 16]> {
    b.try_into()
        .map_err(|_| DbError::Corrupt(format!("id of {} bytes", b.len())))
}

fn parse_ext(s: &str) -> Result<ExternalIds> {
    Ok(serde_json::from_str(s)?)
}

fn item_from_row(r: &Row) -> rusqlite::Result<Result<MediaItem>> {
    let id: Vec<u8> = r.get(0)?;
    let kind: String = r.get(1)?;
    let artist: Option<Vec<u8>> = r.get(3)?;
    let album: Option<Vec<u8>> = r.get(4)?;
    let ext: String = r.get(9)?;
    let title: String = r.get(2)?;
    let (track_number, disc_number, duration_ms, isrc) =
        (r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?);
    Ok((|| {
        Ok(MediaItem {
            id: MediaItemId::from_bytes(to_id(&id)?),
            kind: MediaKind::parse(&kind)
                .ok_or_else(|| DbError::Corrupt(format!("media kind {kind}")))?,
            title,
            artist_id: artist
                .map(|b| to_id(&b).map(ArtistId::from_bytes))
                .transpose()?,
            album_id: album
                .map(|b| to_id(&b).map(AlbumId::from_bytes))
                .transpose()?,
            track_number,
            disc_number,
            duration_ms,
            isrc,
            external_ids: parse_ext(&ext)?,
        })
    })())
}

/// Turns user typing into a safe FTS5 query: each alphanumeric word becomes a
/// quoted prefix term. Returns `None` when nothing searchable was typed.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()?)
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

mod library;
pub use library::{FileFacts, ImportCounts, StoredFile, TrackImport};

#[cfg(test)]
mod tests;
