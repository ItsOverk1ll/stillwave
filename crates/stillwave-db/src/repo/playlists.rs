// SPDX-License-Identifier: GPL-3.0-only
//! Playlists, likes and placeholders as written by imports and read by the
//! playlist screens. An import is one transaction: a crash mid-way leaves the
//! library as it was.

use rusqlite::{params, OptionalExtension, Transaction};
use stillwave_core::{
    EntryTarget, ExternalIds, LikeId, MediaItemId, PlaceholderId, PlaylistEntryId, PlaylistId,
};

use super::browse::{finish_rows, raw_row, LibraryRow, RawRow, ROW_COLUMNS};
use super::{split_target, to_id, Db, DbError, Logger, Result};

/// A library track as the import matcher sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCandidate {
    pub item_id: MediaItemId,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
}

/// A song from an import that is not in the library (yet).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaceholderSpec {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    /// For example `spotify` → `spotify:track:…`; the Spotify URI also dedupes.
    pub external_ids: ExternalIds,
}

/// What one imported entry points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    Item(MediaItemId),
    Missing(PlaceholderSpec),
}

/// A playlist to create, or to replace if `source_key` was imported before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistImport {
    /// Stable name of the source, e.g. `spotify:My Playlist`.
    pub source_key: String,
    pub name: String,
    pub description: Option<String>,
    pub entries: Vec<ImportTarget>,
}

/// A playlist for the list screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub entry_count: u32,
    /// Entries that are in the library (directly or through a resolved placeholder).
    pub owned_count: u32,
}

/// One row of a playlist (or of Liked Songs): a library track, or a song not owned yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryRow {
    Track(LibraryRow),
    Missing {
        placeholder_id: String,
        title: String,
        artist: String,
        album: Option<String>,
        duration_ms: Option<u64>,
    },
}

/// An unresolved placeholder, for matching after a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPlaceholder {
    pub id: PlaceholderId,
    pub spec: PlaceholderSpec,
}

/// The item a placeholder-or-item entry resolves to, if any.
const TARGET_ITEM: &str = "COALESCE(e.media_item_id,
    (SELECT p.resolved_to FROM placeholders p WHERE p.id = e.placeholder_id))";

/// Finds or creates the placeholder for `spec`. Same Spotify URI → same placeholder.
fn placeholder_for(tx: &Transaction, log: &mut Logger, spec: &PlaceholderSpec) -> Result<Vec<u8>> {
    if let Some(uri) = spec.external_ids.get("spotify") {
        let found: Option<Vec<u8>> = tx
            .prepare_cached(
                "SELECT id FROM placeholders WHERE json_extract(external_ids, '$.spotify') = ?1",
            )?
            .query_row(params![uri], |r| r.get(0))
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
    }
    let id = PlaceholderId::new();
    tx.prepare_cached(
        "INSERT INTO placeholders (id, title, artist, album, duration_ms, isrc, external_ids)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        id.as_bytes().as_slice(),
        spec.title,
        spec.artist,
        spec.album,
        spec.duration_ms,
        spec.isrc,
        serde_json::to_string(&spec.external_ids)?
    ])?;
    log.record(
        tx,
        "placeholder",
        id.as_bytes(),
        "insert",
        &[
            "title",
            "artist",
            "album",
            "duration_ms",
            "isrc",
            "external_ids",
        ],
    )?;
    Ok(id.as_bytes().to_vec())
}

/// (media item, placeholder) columns of an entry; exactly one is set.
type TargetIds = (Option<Vec<u8>>, Option<Vec<u8>>);

fn target_ids(tx: &Transaction, log: &mut Logger, t: &ImportTarget) -> Result<TargetIds> {
    Ok(match t {
        ImportTarget::Item(i) => split_target(EntryTarget::Item(*i)),
        ImportTarget::Missing(spec) => (None, Some(placeholder_for(tx, log, spec)?)),
    })
}

/// Sortable position key for the `index`th entry; leaves room to insert between.
fn position(index: usize) -> String {
    format!("{:010}", index * 1000)
}

/// A library row, or the placeholder columns when the entry has no library item.
type RawEntry = (Option<RawRow>, Option<MissingRaw>);

fn map_entry(r: &rusqlite::Row) -> rusqlite::Result<RawEntry> {
    let item: Option<Vec<u8>> = r.get(0)?;
    if item.is_some() {
        return Ok((Some(raw_row(r)?), None));
    }
    Ok((
        None,
        Some(MissingRaw {
            id: r.get(13)?,
            title: r.get(14)?,
            artist: r.get(15)?,
            album: r.get(16)?,
            duration_ms: r.get(17)?,
        }),
    ))
}

struct MissingRaw {
    id: Option<Vec<u8>>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: Option<u64>,
}

fn finish_entries(raw: Vec<RawEntry>) -> Result<Vec<EntryRow>> {
    raw.into_iter()
        .map(|(track, missing)| match (track, missing) {
            (Some(t), _) => Ok(EntryRow::Track(
                finish_rows(vec![t])?
                    .pop()
                    .ok_or_else(|| DbError::Corrupt("empty row".into()))?,
            )),
            (None, Some(m)) => Ok(EntryRow::Missing {
                placeholder_id: PlaceholderId::from_bytes(to_id(
                    &m.id
                        .ok_or_else(|| DbError::Corrupt("entry without target".into()))?,
                )?)
                .to_string(),
                title: m.title.unwrap_or_default(),
                artist: m.artist.unwrap_or_default(),
                album: m.album,
                duration_ms: m.duration_ms,
            }),
            (None, None) => Err(DbError::Corrupt("entry without target".into())),
        })
        .collect()
}

/// Joins for an entry query: `e` has `media_item_id`/`placeholder_id`; `?1` is this device.
const ENTRY_JOINS: &str = "LEFT JOIN media_items m ON m.id = {TARGET}
    LEFT JOIN artists a ON a.id = m.artist_id
    LEFT JOIN albums al ON al.id = m.album_id
    LEFT JOIN media_files f ON f.media_item_id = m.id AND f.device_id = ?1
    LEFT JOIN placeholders ph ON ph.id = e.placeholder_id";

const MISSING_COLUMNS: &str = "ph.id, ph.title, ph.artist, ph.album, ph.duration_ms";

fn entry_sql(from: &str, order: &str) -> String {
    format!(
        "{ROW_COLUMNS}, {MISSING_COLUMNS} FROM {from} {} {order}",
        ENTRY_JOINS.replace("{TARGET}", TARGET_ITEM)
    )
}

impl Db {
    /// Every library track, for matching imports against. 50,000 tracks is a few MB.
    pub fn match_candidates(&self) -> Result<Vec<MatchCandidate>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT m.id, m.title, a.name, al.title, m.duration_ms, m.isrc FROM media_items m
             LEFT JOIN artists a ON a.id = m.artist_id
             LEFT JOIN albums al ON al.id = m.album_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                MatchCandidate {
                    item_id: MediaItemId::from_bytes([0; 16]),
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    duration_ms: r.get(4)?,
                    isrc: r.get(5)?,
                },
            ))
        })?;
        rows.map(|r| {
            let (id, mut c) = r?;
            c.item_id = MediaItemId::from_bytes(to_id(&id)?);
            Ok(c)
        })
        .collect()
    }

    /// Creates the playlist, or replaces the entries of the one imported earlier
    /// from the same `source_key`. Returns its ID.
    pub fn import_playlist(&mut self, p: &PlaylistImport) -> Result<PlaylistId> {
        self.write(|tx, log| {
            let existing: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT id FROM playlists WHERE source_key = ?1",
                    params![p.source_key],
                    |r| r.get(0),
                )
                .optional()?;
            let id = if let Some(raw) = existing {
                let id = PlaylistId::from_bytes(to_id(&raw)?);
                tx.execute(
                    "UPDATE playlists SET name = ?2, description = ?3 WHERE id = ?1",
                    params![raw, p.name, p.description],
                )?;
                log.record(tx, "playlist", id.as_bytes(), "update", &["name", "description"])?;
                let old: Vec<Vec<u8>> = tx
                    .prepare("SELECT id FROM playlist_entries WHERE playlist_id = ?1")?
                    .query_map(params![raw], |r| r.get(0))?
                    .collect::<std::result::Result<_, _>>()?;
                tx.execute(
                    "DELETE FROM playlist_entries WHERE playlist_id = ?1",
                    params![raw],
                )?;
                for e in old {
                    log.record_raw(tx, "playlist_entry", &e, "delete", &[])?;
                }
                id
            } else {
                let id = PlaylistId::new();
                tx.execute(
                    "INSERT INTO playlists (id, name, description, source_key) VALUES (?1, ?2, ?3, ?4)",
                    params![id.as_bytes().as_slice(), p.name, p.description, p.source_key],
                )?;
                log.record(tx, "playlist", id.as_bytes(), "insert", &["name", "description"])?;
                id
            };
            for (i, t) in p.entries.iter().enumerate() {
                let (item, ph) = target_ids(tx, log, t)?;
                let eid = PlaylistEntryId::new();
                tx.prepare_cached(
                    "INSERT INTO playlist_entries (id, playlist_id, media_item_id, placeholder_id, position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?
                .execute(params![
                    eid.as_bytes().as_slice(),
                    id.as_bytes().as_slice(),
                    item,
                    ph,
                    position(i)
                ])?;
                log.record(tx, "playlist_entry", eid.as_bytes(), "insert", &["playlist_id", "target", "position"])?;
            }
            Ok(id)
        })
    }

    /// Adds likes in the given order (first is oldest). Already-liked songs are kept
    /// as they are. Returns how many were new.
    pub fn import_likes(&mut self, targets: &[ImportTarget]) -> Result<u32> {
        self.write(|tx, log| {
            let mut added = 0;
            for t in targets {
                let (item, ph) = target_ids(tx, log, t)?;
                let id = LikeId::new();
                let hlc = log.clock.tick();
                let n = tx
                    .prepare_cached(
                        "INSERT OR IGNORE INTO likes (id, media_item_id, placeholder_id, liked_at)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![id.as_bytes().as_slice(), item, ph, hlc.to_string()])?;
                if n > 0 {
                    added += 1;
                    log.record(tx, "like", id.as_bytes(), "insert", &["target", "liked_at"])?;
                }
            }
            Ok(added)
        })
    }

    /// All playlists by name.
    pub fn playlists(&self) -> Result<Vec<PlaylistRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT pl.id, pl.name, pl.description,
                (SELECT COUNT(*) FROM playlist_entries e WHERE e.playlist_id = pl.id),
                (SELECT COUNT(*) FROM playlist_entries e WHERE e.playlist_id = pl.id
                    AND {TARGET_ITEM} IS NOT NULL)
             FROM playlists pl ORDER BY pl.name COLLATE NOCASE, pl.id"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                PlaylistRow {
                    id: String::new(),
                    name: r.get(1)?,
                    description: r.get(2)?,
                    entry_count: r.get(3)?,
                    owned_count: r.get(4)?,
                },
            ))
        })?;
        rows.map(|r| {
            let (id, mut p) = r?;
            p.id = PlaylistId::from_bytes(to_id(&id)?).to_string();
            Ok(p)
        })
        .collect()
    }

    /// One playlist, or `None` if the ID is unknown.
    pub fn playlist(&self, id: &str) -> Result<Option<PlaylistRow>> {
        let id = parse_playlist(id)?.to_string();
        Ok(self.playlists()?.into_iter().find(|p| p.id == id))
    }

    /// A playlist's entries in order.
    pub fn playlist_entries(&self, id: &str) -> Result<Vec<EntryRow>> {
        let id = parse_playlist(id)?;
        let mut stmt = self.conn.prepare_cached(&entry_sql(
            "playlist_entries e",
            "WHERE e.playlist_id = ?2 ORDER BY e.position, e.id",
        ))?;
        let rows = stmt
            .query_map(
                params![self.device.as_bytes().as_slice(), id.as_bytes().as_slice()],
                map_entry,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        finish_entries(rows)
    }

    /// Liked songs, oldest like first.
    pub fn liked_entries(&self) -> Result<Vec<EntryRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(&entry_sql("likes e", "ORDER BY e.liked_at, e.id"))?;
        let rows = stmt
            .query_map(params![self.device.as_bytes().as_slice()], map_entry)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        finish_entries(rows)
    }

    /// Number of likes, and how many of them are in the library.
    pub fn like_counts(&self) -> Result<(u32, u32)> {
        Ok(self.conn.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM({TARGET_ITEM} IS NOT NULL), 0) FROM likes e"),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Placeholders not yet matched to a library item.
    pub fn open_placeholders(&self) -> Result<Vec<OpenPlaceholder>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, artist, album, duration_ms, isrc, external_ids
             FROM placeholders WHERE resolved_to IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(6)?,
                PlaceholderSpec {
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    duration_ms: r.get(4)?,
                    isrc: r.get(5)?,
                    external_ids: ExternalIds::new(),
                },
            ))
        })?;
        rows.map(|r| {
            let (id, ext, mut spec) = r?;
            spec.external_ids = serde_json::from_str(&ext)?;
            Ok(OpenPlaceholder {
                id: PlaceholderId::from_bytes(to_id(&id)?),
                spec,
            })
        })
        .collect()
    }

    /// Records that placeholders are now in the library (for example after a scan
    /// found the songs). Playlists and likes pointing at them become playable.
    pub fn resolve_placeholders(&mut self, found: &[(PlaceholderId, MediaItemId)]) -> Result<()> {
        if found.is_empty() {
            return Ok(());
        }
        self.write(|tx, log| {
            for (ph, item) in found {
                tx.prepare_cached("UPDATE placeholders SET resolved_to = ?2 WHERE id = ?1")?
                    .execute(params![
                        ph.as_bytes().as_slice(),
                        item.as_bytes().as_slice()
                    ])?;
                log.record(tx, "placeholder", ph.as_bytes(), "update", &["resolved_to"])?;
            }
            Ok(())
        })
    }
}

fn parse_playlist(id: &str) -> Result<PlaylistId> {
    id.parse()
        .map_err(|e: stillwave_core::ParseIdError| DbError::Corrupt(e.to_string()))
}
