// SPDX-License-Identifier: GPL-3.0-only
//! Scan-facing writes: import tracks found on disk, and drop files that vanished.
//!
//! One media item per file for now. A file that disappears loses its `media_files`
//! row but its media item stays, so playlists and likes that point at it survive
//! a drive being unplugged.

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Transaction};
use stillwave_core::{AlbumId, ArtistId, MediaFileId, MediaItemId};

use super::{Db, Logger, Result};

/// Facts about a file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    pub path: String,
    pub size_bytes: u64,
    pub modified_unix_ms: i64,
    pub container: String,
    pub codec: String,
    pub sample_rate_hz: u32,
    pub bit_depth: Option<u8>,
    pub channels: u8,
    pub duration_ms: u64,
}

/// A track as read from a file's tags, ready to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackImport {
    pub title: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub isrc: Option<String>,
    pub file: FileFacts,
}

/// What the database already knows about a stored file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredFile {
    pub size_bytes: u64,
    pub modified_unix_ms: i64,
}

/// Outcome of an import batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub added: u32,
    pub updated: u32,
}

impl Db {
    /// Every file stored for this device, keyed by path.
    pub fn stored_files(&self) -> Result<HashMap<String, StoredFile>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, size_bytes, modified_unix_ms FROM media_files WHERE device_id = ?1",
        )?;
        let rows = stmt.query_map(params![self.device.as_bytes().as_slice()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                StoredFile {
                    size_bytes: r.get(1)?,
                    modified_unix_ms: r.get(2)?,
                },
            ))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Imports a batch in ONE transaction. A path already stored is updated in
    /// place (same item and file IDs); a new path creates an item and a file.
    pub fn import_tracks(&mut self, batch: &[TrackImport]) -> Result<ImportCounts> {
        let device = self.device;
        self.write(|tx, log| {
            let mut counts = ImportCounts::default();
            for t in batch {
                let existing = tx
                    .query_row(
                        "SELECT id, media_item_id FROM media_files WHERE device_id = ?1 AND path = ?2",
                        params![device.as_bytes().as_slice(), t.file.path],
                        |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)),
                    )
                    .optional()?;
                let artist = t
                    .artist
                    .as_deref()
                    .or(t.album_artist.as_deref())
                    .map(|n| find_or_create_artist(tx, log, n))
                    .transpose()?;
                let album_artist = t
                    .album_artist
                    .as_deref()
                    .or(t.artist.as_deref())
                    .map(|n| find_or_create_artist(tx, log, n))
                    .transpose()?;
                let album = t
                    .album
                    .as_deref()
                    .map(|a| find_or_create_album(tx, log, a, album_artist, t.year))
                    .transpose()?;
                let (artist, album) = (
                    artist.map(|a| a.as_bytes().to_vec()),
                    album.map(|a| a.as_bytes().to_vec()),
                );
                if let Some((file_id, item_id)) = existing {
                    update_item(tx, &item_id, t, artist.as_deref(), album.as_deref())?;
                    update_file(tx, &file_id, &t.file)?;
                    log.record_raw(tx, "media_item", &item_id, "update", &["title", "artist_id", "album_id", "track_number", "disc_number", "duration_ms", "isrc"])?;
                    log.record_raw(tx, "media_file", &file_id, "update", &["size_bytes", "modified_unix_ms", "codec", "duration_ms"])?;
                    counts.updated += 1;
                } else {
                    let item_id = MediaItemId::new();
                    let file_id = MediaFileId::new();
                    tx.execute(
                        "INSERT INTO media_items (id, kind, title, artist_id, album_id, track_number,
                                                  disc_number, duration_ms, isrc, external_ids)
                         VALUES (?1, 'track', ?2, ?3, ?4, ?5, ?6, ?7, ?8, '{}')",
                        params![
                            item_id.as_bytes().as_slice(),
                            t.title,
                            artist,
                            album,
                            t.track_number,
                            t.disc_number,
                            t.file.duration_ms,
                            t.isrc
                        ],
                    )?;
                    insert_file(tx, &file_id, &item_id, device.as_bytes(), &t.file)?;
                    log.record_raw(tx, "media_item", item_id.as_bytes(), "insert", &["kind", "title", "artist_id", "album_id", "track_number", "disc_number", "duration_ms", "isrc"])?;
                    log.record_raw(tx, "media_file", file_id.as_bytes(), "insert", &["media_item_id", "path", "size_bytes", "modified_unix_ms", "codec"])?;
                    counts.added += 1;
                }
            }
            Ok(counts)
        })
    }

    /// Removes the file rows for `paths` (this device). Media items are kept.
    pub fn remove_files(&mut self, paths: &[String]) -> Result<u32> {
        let device = self.device;
        self.write(|tx, log| {
            let mut removed = 0;
            for p in paths {
                let id: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT id FROM media_files WHERE device_id = ?1 AND path = ?2",
                        params![device.as_bytes().as_slice(), p],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(id) = id {
                    tx.execute("DELETE FROM media_files WHERE id = ?1", params![id])?;
                    log.record_raw(tx, "media_file", &id, "delete", &[])?;
                    removed += 1;
                }
            }
            Ok(removed)
        })
    }

    /// Number of file rows stored for this device.
    pub fn file_count(&self) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM media_files WHERE device_id = ?1",
            params![self.device.as_bytes().as_slice()],
            |r| r.get(0),
        )?)
    }
}

fn find_or_create_artist(tx: &Transaction, log: &mut Logger, name: &str) -> Result<ArtistId> {
    let found: Option<Vec<u8>> = tx
        .query_row(
            "SELECT id FROM artists WHERE name = ?1 COLLATE NOCASE LIMIT 1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(b) = found {
        return Ok(ArtistId::from_bytes(super::to_id(&b)?));
    }
    let id = ArtistId::new();
    tx.execute(
        "INSERT INTO artists (id, name, external_ids) VALUES (?1, ?2, '{}')",
        params![id.as_bytes().as_slice(), name],
    )?;
    log.record_raw(tx, "artist", id.as_bytes(), "insert", &["name"])?;
    Ok(id)
}

fn find_or_create_album(
    tx: &Transaction,
    log: &mut Logger,
    title: &str,
    artist: Option<ArtistId>,
    year: Option<i32>,
) -> Result<AlbumId> {
    let artist_bytes = artist.map(|a| a.as_bytes().to_vec());
    let found: Option<Vec<u8>> = tx
        .query_row(
            "SELECT id FROM albums WHERE title = ?1 COLLATE NOCASE AND album_artist_id IS ?2 LIMIT 1",
            params![title, artist_bytes],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(b) = found {
        return Ok(AlbumId::from_bytes(super::to_id(&b)?));
    }
    let id = AlbumId::new();
    tx.execute(
        "INSERT INTO albums (id, title, album_artist_id, year, external_ids) VALUES (?1, ?2, ?3, ?4, '{}')",
        params![id.as_bytes().as_slice(), title, artist_bytes, year],
    )?;
    log.record_raw(
        tx,
        "album",
        id.as_bytes(),
        "insert",
        &["title", "album_artist_id", "year"],
    )?;
    Ok(id)
}

fn update_item(
    tx: &Transaction,
    item_id: &[u8],
    t: &TrackImport,
    artist: Option<&[u8]>,
    album: Option<&[u8]>,
) -> Result<()> {
    tx.execute(
        "UPDATE media_items SET title = ?2, artist_id = ?3, album_id = ?4, track_number = ?5,
                disc_number = ?6, duration_ms = ?7, isrc = ?8 WHERE id = ?1",
        params![
            item_id,
            t.title,
            artist,
            album,
            t.track_number,
            t.disc_number,
            t.file.duration_ms,
            t.isrc
        ],
    )?;
    Ok(())
}

fn update_file(tx: &Transaction, file_id: &[u8], f: &FileFacts) -> Result<()> {
    tx.execute(
        "UPDATE media_files SET size_bytes = ?2, modified_unix_ms = ?3, container = ?4, codec = ?5,
                sample_rate_hz = ?6, bit_depth = ?7, channels = ?8, duration_ms = ?9 WHERE id = ?1",
        params![
            file_id,
            f.size_bytes,
            f.modified_unix_ms,
            f.container,
            f.codec,
            f.sample_rate_hz,
            f.bit_depth,
            f.channels,
            f.duration_ms
        ],
    )?;
    Ok(())
}

fn insert_file(
    tx: &Transaction,
    file_id: &MediaFileId,
    item_id: &MediaItemId,
    device: &[u8; 16],
    f: &FileFacts,
) -> Result<()> {
    tx.execute(
        "INSERT INTO media_files (id, media_item_id, device_id, path, size_bytes, modified_unix_ms,
                                  container, codec, sample_rate_hz, bit_depth, channels, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            file_id.as_bytes().as_slice(),
            item_id.as_bytes().as_slice(),
            device.as_slice(),
            f.path,
            f.size_bytes,
            f.modified_unix_ms,
            f.container,
            f.codec,
            f.sample_rate_hz,
            f.bit_depth,
            f.channels,
            f.duration_ms
        ],
    )?;
    Ok(())
}
