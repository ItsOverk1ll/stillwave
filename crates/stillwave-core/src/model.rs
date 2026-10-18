// SPDX-License-Identifier: GPL-3.0-only
//! Domain entities. These are plain data; persistence lives in `stillwave-db`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hlc::Hlc;
use crate::ids::{
    AlbumId, ArtistId, DeviceId, LikeId, MediaFileId, MediaItemId, PlaceholderId, PlayId,
    PlaylistEntryId, PlaylistId,
};

/// What kind of thing a [`MediaItem`] is. Music first; the rest are reserved so
/// the schema never has to change shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Track,
    PodcastEpisode,
    AudiobookChapter,
    Video,
}

impl MediaKind {
    /// Stable string stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::PodcastEpisode => "podcast_episode",
            Self::AudiobookChapter => "audiobook_chapter",
            Self::Video => "video",
        }
    }

    /// Inverse of [`as_str`](Self::as_str).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "track" => Self::Track,
            "podcast_episode" => Self::PodcastEpisode,
            "audiobook_chapter" => Self::AudiobookChapter,
            "video" => Self::Video,
            _ => return None,
        })
    }
}

/// IDs in other systems (ISRC, Spotify URI, `MusicBrainz`, ...), keyed by scheme.
/// Kept on every entity that can be matched during an import.
pub type ExternalIds = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub sort_name: Option<String>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub album_artist_id: Option<ArtistId>,
    pub year: Option<i32>,
    pub external_ids: ExternalIds,
}

/// A generic playable thing in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: MediaItemId,
    pub kind: MediaKind,
    pub title: String,
    pub artist_id: Option<ArtistId>,
    pub album_id: Option<AlbumId>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    pub external_ids: ExternalIds,
}

/// A file on one device that backs a [`MediaItem`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: MediaFileId,
    pub media_item_id: MediaItemId,
    pub device_id: DeviceId,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    pub description: Option<String>,
}

/// What a playlist entry points at: a real item, or a song the user does not
/// own yet (so imported playlists keep their full shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum EntryTarget {
    Item(MediaItemId),
    Placeholder(PlaceholderId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub id: PlaylistEntryId,
    pub playlist_id: PlaylistId,
    pub target: EntryTarget,
    /// Fractional-index style key so reordering never rewrites other rows.
    pub position: String,
}

/// A song that is not in the library yet, created by imports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placeholder {
    pub id: PlaceholderId,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    /// e.g. `spotify` -> `spotify:track:...`
    pub external_ids: ExternalIds,
    /// Set once the placeholder has been matched to a library item.
    pub resolved_to: Option<MediaItemId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Play {
    pub id: PlayId,
    pub media_item_id: MediaItemId,
    pub device_id: DeviceId,
    pub started_unix_ms: i64,
    pub played_ms: u64,
    pub completed: bool,
}

/// A like on either a library item or a placeholder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Like {
    pub id: LikeId,
    pub target: EntryTarget,
    pub liked_at: Hlc,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_kind_round_trips() {
        for k in [
            MediaKind::Track,
            MediaKind::PodcastEpisode,
            MediaKind::AudiobookChapter,
            MediaKind::Video,
        ] {
            assert_eq!(MediaKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(MediaKind::parse("nope"), None);
    }

    #[test]
    fn placeholder_serializes() {
        let mut ext = ExternalIds::new();
        ext.insert("spotify".into(), "spotify:track:abc".into());
        let p = Placeholder {
            id: PlaceholderId::new(),
            title: "Song".into(),
            artist: "Band".into(),
            album: None,
            duration_ms: Some(180_000),
            isrc: Some("USRC17607839".into()),
            external_ids: ext,
            resolved_to: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Placeholder>(&json).unwrap(), p);
    }

    #[test]
    fn entry_target_serializes_tagged() {
        let t = EntryTarget::Item(MediaItemId::new());
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"type\":\"item\""));
        assert_eq!(serde_json::from_str::<EntryTarget>(&json).unwrap(), t);
    }
}
