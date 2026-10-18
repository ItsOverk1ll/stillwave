// SPDX-License-Identifier: GPL-3.0-only
//! Reads one file's tags and audio properties.

use std::fs::File;
use std::path::Path;
use std::time::UNIX_EPOCH;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::tag::{Accessor, ItemKey, Tag};
use stillwave_db::{FileFacts, TrackImport};

/// Extensions the scanner looks at (lower case, no dot).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "wave", "ogg", "oga", "opus", "m4a", "m4b", "aac", "aif", "aiff",
];

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("cannot read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a readable audio file: {0}")]
    Parse(String),
    #[error("file has no audio properties (sample rate or channels)")]
    NoProperties,
}

/// Reads `path` into a [`TrackImport`]. The title falls back to the file name.
pub fn read_track(path: &Path) -> Result<TrackImport, ReadError> {
    let meta = std::fs::metadata(path)?;
    let modified_unix_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);

    let tagged = lofty::read_from_path(path).map_err(|e| ReadError::Parse(e.to_string()))?;
    let props = tagged.properties();
    let (Some(sample_rate_hz), Some(channels)) = (props.sample_rate(), props.channels()) else {
        return Err(ReadError::NoProperties);
    };
    let file_type = tagged.file_type();
    let (container, codec) = container_and_codec(path, file_type);

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let text = |key: ItemKey| {
        tag.and_then(|t| t.get_string(key))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let title = text(ItemKey::TrackTitle).unwrap_or_else(|| {
        path.file_stem()
            .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
    });

    Ok(TrackImport {
        title,
        artist: text(ItemKey::TrackArtist),
        album_artist: text(ItemKey::AlbumArtist),
        album: text(ItemKey::AlbumTitle),
        year: year(tag),
        track_number: tag.and_then(Accessor::track).filter(|n| *n > 0),
        disc_number: tag.and_then(Accessor::disk).filter(|n| *n > 0),
        isrc: text(ItemKey::Isrc),
        file: FileFacts {
            path: path.to_string_lossy().into_owned(),
            size_bytes: meta.len(),
            modified_unix_ms,
            container,
            codec,
            sample_rate_hz,
            bit_depth: props.bit_depth(),
            channels,
            duration_ms: u64::try_from(props.duration().as_millis()).unwrap_or(u64::MAX),
        },
    })
}

fn year(tag: Option<&Tag>) -> Option<i32> {
    let tag = tag?;
    [ItemKey::RecordingDate, ItemKey::Year]
        .into_iter()
        .find_map(|k| tag.get_string(k))
        .and_then(|s| s.trim().get(..4))
        .and_then(|y| y.parse().ok())
        .filter(|y| (1000..=2999).contains(y))
}

fn container_and_codec(path: &Path, file_type: FileType) -> (String, String) {
    let (container, codec) = match file_type {
        FileType::Flac => ("flac", "flac"),
        FileType::Mpeg => ("mp3", "mp3"),
        FileType::Wav => ("wav", "pcm"),
        FileType::Aiff => ("aiff", "pcm"),
        FileType::Opus => ("ogg", "opus"),
        FileType::Vorbis => ("ogg", "vorbis"),
        FileType::Aac => ("aac", "aac"),
        FileType::Mp4 => ("mp4", mp4_codec(path)),
        _ => ("other", "unknown"),
    };
    (container.into(), codec.into())
}

/// M4A can hold lossless ALAC or lossy AAC, and the difference matters for
/// the quality badge, so ask the MP4 parser.
fn mp4_codec(path: &Path) -> &'static str {
    let Ok(mut file) = File::open(path) else {
        return "aac";
    };
    match Mp4File::read_from(&mut file, ParseOptions::new())
        .ok()
        .and_then(|f| f.properties().codec())
    {
        Some(Mp4Codec::ALAC) => "alac",
        Some(Mp4Codec::FLAC) => "flac",
        Some(Mp4Codec::MP3) => "mp3",
        _ => "aac",
    }
}
