// SPDX-License-Identifier: GPL-3.0-only
//! Imports Spotify playlists and liked songs into the library: each song is
//! matched to a library track, and songs the user does not own become
//! placeholders, so playlists keep their full shape and fill in as music is added.

use std::path::PathBuf;

use stillwave_core::ExternalIds;
use stillwave_db::{Db, DbError, ImportTarget, PlaceholderSpec, PlaylistImport};

use crate::matching::{Matcher, Query};
use crate::spotify::{read_export, SpotifyTrack};

/// How one playlist went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistResult {
    pub id: String,
    pub name: String,
    pub total: u32,
    /// Songs found in the library.
    pub matched: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub playlists: Vec<PlaylistResult>,
    /// Liked songs: (total, found in library), if the export had them.
    pub liked: Option<(u32, u32)>,
    /// Podcast episodes and audiobooks left out.
    pub skipped: u32,
    /// Files that could not be read: (file, reason).
    pub problems: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    NothingFound(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

fn target(m: &Matcher, t: &SpotifyTrack) -> ImportTarget {
    let q = Query {
        title: &t.title,
        artist: &t.artist,
        album: t.album.as_deref(),
        duration_ms: t.duration_ms,
        isrc: t.isrc.as_deref(),
    };
    if let Some(id) = m.find(&q) {
        return ImportTarget::Item(id);
    }
    let mut spec = PlaceholderSpec {
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: t.duration_ms,
        isrc: t.isrc.clone(),
        external_ids: ExternalIds::default(),
    };
    if let Some(uri) = &t.uri {
        spec.external_ids.insert("spotify".into(), uri.clone());
    }
    ImportTarget::Missing(spec)
}

fn count_matched(targets: &[ImportTarget]) -> u32 {
    u32::try_from(
        targets
            .iter()
            .filter(|t| matches!(t, ImportTarget::Item(_)))
            .count(),
    )
    .unwrap_or(u32::MAX)
}

/// Imports everything recognised under `paths` (an unzipped Spotify data download,
/// or CSV playlist files). Re-importing the same playlists updates them.
pub fn import_spotify(db: &mut Db, paths: &[PathBuf]) -> Result<ImportSummary, ImportError> {
    let export = read_export(paths);
    let mut summary = ImportSummary {
        problems: export
            .problems
            .iter()
            .map(|(p, e)| (p.display().to_string(), e.clone()))
            .collect(),
        ..ImportSummary::default()
    };
    if export.playlists.is_empty() && export.liked.is_none() {
        let why = summary
            .problems
            .first()
            .map(|(f, e)| format!(" {f}: {e}"))
            .unwrap_or_default();
        return Err(ImportError::NothingFound(format!(
            "No Spotify playlists found there. Choose the folder from your Spotify data \
             download (it contains Playlist1.json) or a CSV playlist export.{why}"
        )));
    }

    let matcher = Matcher::new(db.match_candidates()?);
    for p in &export.playlists {
        let entries: Vec<ImportTarget> = p.tracks.iter().map(|t| target(&matcher, t)).collect();
        let owned = count_matched(&entries);
        let total = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        let playlist_id = db.import_playlist(&PlaylistImport {
            source_key: p.source_key.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            entries,
        })?;
        summary.skipped += p.skipped;
        summary.playlists.push(PlaylistResult {
            id: playlist_id.to_string(),
            name: p.name.clone(),
            total,
            matched: owned,
        });
    }
    if let Some(liked) = &export.liked {
        let targets: Vec<ImportTarget> = liked.iter().map(|t| target(&matcher, t)).collect();
        db.import_likes(&targets)?;
        summary.liked = Some((
            u32::try_from(targets.len()).unwrap_or(u32::MAX),
            count_matched(&targets),
        ));
    }
    Ok(summary)
}

/// Matches placeholders against the library again (after a scan added music).
/// Returns how many are now in the library.
pub fn resolve_placeholders(db: &mut Db) -> Result<u32, DbError> {
    let open = db.open_placeholders()?;
    if open.is_empty() {
        return Ok(0);
    }
    let matcher = Matcher::new(db.match_candidates()?);
    let found: Vec<_> = open
        .iter()
        .filter_map(|p| {
            matcher
                .find(&Query {
                    title: &p.spec.title,
                    artist: &p.spec.artist,
                    album: p.spec.album.as_deref(),
                    duration_ms: p.spec.duration_ms,
                    isrc: p.spec.isrc.as_deref(),
                })
                .map(|item| (p.id, item))
        })
        .collect();
    db.resolve_placeholders(&found)?;
    Ok(u32::try_from(found.len()).unwrap_or(u32::MAX))
}
