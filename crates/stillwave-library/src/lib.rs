// SPDX-License-Identifier: GPL-3.0-only
//! Library scanner. Finds audio files, reads tags and properties with Lofty and
//! stores them through `stillwave-db`. Reading is strictly read-only: this crate
//! never writes to a user's audio files.
//!
//! Scanning is blocking and meant to run on a worker thread (never the UI thread);
//! it reports progress through a callback and stops promptly when cancelled.

mod import;
pub mod matching;
mod read;
mod scan;
pub mod spotify;

pub use import::{
    import_spotify, resolve_placeholders, ImportError, ImportSummary, PlaylistResult,
};
pub use read::{read_track, ReadError, AUDIO_EXTENSIONS};
pub use scan::{scan, LibraryStore, ScanError, ScanOptions, ScanProgress, ScanReport};
