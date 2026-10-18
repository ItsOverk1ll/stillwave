// SPDX-License-Identifier: GPL-3.0-only
//! Folder scan: walk, skip unchanged files, read the rest, import in batches.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use stillwave_db::{Db, DbError, ImportCounts, StoredFile, TrackImport};
use walkdir::WalkDir;

use crate::read::{read_track, AUDIO_EXTENSIONS};

/// Files per database transaction. Big enough to be fast, small enough that a
/// cancelled scan keeps most of its work and progress stays smooth.
const BATCH: usize = 250;

/// Where a scan reads and writes library data.
///
/// `Db` implements this directly. An app that shares one database between a scan
/// and the UI implements it over its own lock and takes the lock **per call**, so a
/// long scan never freezes search or playback.
pub trait LibraryStore {
    /// Files already stored for this device, keyed by path.
    fn stored_files(&mut self) -> Result<HashMap<String, StoredFile>, DbError>;
    /// Stores a batch in one transaction.
    fn import_tracks(&mut self, batch: &[TrackImport]) -> Result<ImportCounts, DbError>;
    /// Forgets files that no longer exist; returns how many were removed.
    fn remove_files(&mut self, paths: &[String]) -> Result<u32, DbError>;
}

impl LibraryStore for Db {
    fn stored_files(&mut self) -> Result<HashMap<String, StoredFile>, DbError> {
        Db::stored_files(self)
    }
    fn import_tracks(&mut self, batch: &[TrackImport]) -> Result<ImportCounts, DbError> {
        Db::import_tracks(self, batch)
    }
    fn remove_files(&mut self, paths: &[String]) -> Result<u32, DbError> {
        Db::remove_files(self, paths)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan folder does not exist or is not a directory: {0}")]
    BadRoot(PathBuf),
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Set to `true` from another thread to stop the scan after the current file.
    pub cancel: Arc<AtomicBool>,
    /// Re-read every file even if its size and modified time are unchanged.
    pub force: bool,
    /// Remove stored files under the root that no longer exist on disk.
    pub remove_missing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProgress {
    /// Walking the folder; `found` audio files so far.
    Discovering { found: usize },
    /// Reading and storing; `done` of `total` files that need work.
    Importing { done: usize, total: usize },
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub found: usize,
    pub added: u32,
    pub updated: u32,
    pub unchanged: usize,
    pub removed: u32,
    /// Files that could not be read, with the reason. The scan carries on.
    pub failed: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

/// Scans `root`, storing what it finds in `db`.
///
/// A file is re-read only when its size or modified time differs from what the
/// database stored. Unreadable files are reported, never fatal.
pub fn scan<S: LibraryStore>(
    db: &mut S,
    root: &Path,
    options: &ScanOptions,
    mut progress: impl FnMut(ScanProgress),
) -> Result<ScanReport, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::BadRoot(root.to_path_buf()));
    }
    let mut report = ScanReport::default();
    let stored = db.stored_files()?;

    // Pass 1: discover.
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if options.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return Ok(report);
        }
        if entry.file_type().is_file() && is_audio(entry.path()) {
            files.push(entry.into_path());
            if files.len().is_multiple_of(200) {
                progress(ScanProgress::Discovering { found: files.len() });
            }
        }
    }
    report.found = files.len();
    progress(ScanProgress::Discovering { found: files.len() });

    // Pass 2: decide what needs reading.
    let mut todo: Vec<&PathBuf> = Vec::new();
    for path in &files {
        let key = path.to_string_lossy();
        let unchanged = !options.force
            && stored.get(key.as_ref()).is_some_and(|s| {
                std::fs::metadata(path).is_ok_and(|m| {
                    m.len() == s.size_bytes && mtime_ms(&m) == Some(s.modified_unix_ms)
                })
            });
        if unchanged {
            report.unchanged += 1;
        } else {
            todo.push(path);
        }
    }

    // Pass 3: read and import in batches.
    let total = todo.len();
    let mut batch: Vec<TrackImport> = Vec::with_capacity(BATCH);
    for (i, path) in todo.into_iter().enumerate() {
        if options.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        match read_track(path) {
            Ok(t) => batch.push(t),
            Err(e) => report.failed.push((path.clone(), e.to_string())),
        }
        if batch.len() >= BATCH {
            flush(db, &mut batch, &mut report)?;
        }
        if (i + 1).is_multiple_of(25) || i + 1 == total {
            progress(ScanProgress::Importing { done: i + 1, total });
        }
    }
    flush(db, &mut batch, &mut report)?;

    // Pass 4: forget files that disappeared (only after a complete walk).
    if options.remove_missing && !report.cancelled {
        let present: HashSet<String> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let root_prefix = root.to_string_lossy().into_owned();
        let gone: Vec<String> = stored
            .keys()
            .filter(|k| k.starts_with(&root_prefix) && !present.contains(*k))
            .cloned()
            .collect();
        report.removed = db.remove_files(&gone)?;
    }
    Ok(report)
}

fn flush<S: LibraryStore>(
    db: &mut S,
    batch: &mut Vec<TrackImport>,
    report: &mut ScanReport,
) -> Result<(), ScanError> {
    if batch.is_empty() {
        return Ok(());
    }
    let counts = db.import_tracks(batch)?;
    report.added += counts.added;
    report.updated += counts.updated;
    batch.clear();
    Ok(())
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn mtime_ms(m: &std::fs::Metadata) -> Option<i64> {
    m.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
}
