// SPDX-License-Identifier: GPL-3.0-only
//! Strongly typed `UUIDv7` identifiers. Distinct newtypes stop an `AlbumId` being
//! passed where a `TrackId` is expected.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Error returned when an identifier cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid id: {0}")]
pub struct ParseIdError(String);

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new time-ordered (`UUIDv7`) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }

            /// The 16 raw bytes, for compact storage.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            /// Builds an identifier from 16 raw bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(Uuid::from_bytes(bytes))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.hyphenated(), f)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|e| ParseIdError(e.to_string()))
            }
        }
    };
}

define_id!(
    /// Identifies a generic media item (track, episode, chapter, ...).
    MediaItemId
);
define_id!(
    /// Identifies an artist.
    ArtistId
);
define_id!(
    /// Identifies an album.
    AlbumId
);
define_id!(
    /// Identifies a file on a device.
    MediaFileId
);
define_id!(
    /// Identifies a playlist.
    PlaylistId
);
define_id!(
    /// Identifies one entry in a playlist.
    PlaylistEntryId
);
define_id!(
    /// Identifies a placeholder (a song not in the library).
    PlaceholderId
);
define_id!(
    /// Identifies one play event.
    PlayId
);
define_id!(
    /// Identifies a like.
    LikeId
);
define_id!(
    /// Identifies a device taking part in sync.
    DeviceId
);

/// Alias: a track is the first kind of media item.
pub type TrackId = MediaItemId;

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> Uuid {
        Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, 0))
    }

    #[test]
    fn ids_are_version_7() {
        assert_eq!(TrackId::new().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(PlaylistId::new(), PlaylistId::new());
    }

    #[test]
    fn ids_sort_by_creation_time() {
        assert!(AlbumId::from_uuid(at(1_000)) < AlbumId::from_uuid(at(2_000)));
    }

    #[test]
    fn display_and_parse_round_trip() {
        let id = ArtistId::new();
        assert_eq!(id.to_string().parse::<ArtistId>().unwrap(), id);
    }

    #[test]
    fn bytes_round_trip() {
        let id = DeviceId::new();
        assert_eq!(DeviceId::from_bytes(*id.as_bytes()), id);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!("not-a-uuid".parse::<PlayId>().is_err());
    }

    #[test]
    fn serializes_as_plain_string() {
        let id = LikeId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<LikeId>(&json).unwrap(), id);
    }
}
