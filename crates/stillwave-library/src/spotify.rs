// SPDX-License-Identifier: GPL-3.0-only
//! Reads playlists and liked songs from files the user downloaded from Spotify:
//! - the account data download ("Download your data" in Spotify's privacy
//!   settings): `Playlist1.json`, `Playlist2.json`… and `YourLibrary.json`;
//! - CSV playlist exports (one playlist per file), such as Exportify's.
//!
//! Only song metadata is read. Nothing here touches Spotify's servers or audio.
//! The formats are not documented by Spotify, so parsing is lenient: unknown
//! fields are ignored and a file that cannot be read is reported, not fatal.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One song named by an export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpotifyTrack {
    pub title: String,
    /// As given; may name several artists ("A, B").
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<String>,
    /// `spotify:track:…` (or `spotify:local:…`).
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpotifyPlaylist {
    /// Stable identity for re-imports (see `PlaylistImport::source_key`).
    pub source_key: String,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<SpotifyTrack>,
    /// Podcast episodes and audiobooks, which are not imported.
    pub skipped: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpotifyExport {
    pub playlists: Vec<SpotifyPlaylist>,
    /// Liked songs, if the export had them.
    pub liked: Option<Vec<SpotifyTrack>>,
    /// Files that looked relevant but could not be read, with the reason.
    pub problems: Vec<(PathBuf, String)>,
}

/// How deep to look inside a chosen folder (the download unzips to a nested folder).
const MAX_DEPTH: usize = 4;

/// Reads every recognised file under `paths` (files or folders).
#[must_use]
pub fn read_export(paths: &[PathBuf]) -> SpotifyExport {
    let mut files = Vec::new();
    for p in paths {
        collect(p, 0, &mut files);
    }
    files.sort();
    files.dedup();

    let mut out = SpotifyExport::default();
    for f in files {
        let result = match kind(&f) {
            Some(Kind::Zip) => {
                Err("this is still zipped; unzip it first and choose the folder inside".to_owned())
            }
            Some(Kind::Csv) => read_csv_playlist(&f).map(|p| out.playlists.push(p)),
            Some(Kind::Library) => read_json(&f).map(|v| out.liked = Some(parse_library(&v))),
            Some(Kind::Playlists) => {
                read_json(&f).map(|v| out.playlists.extend(parse_playlists(&v)))
            }
            None => Ok(()),
        };
        if let Err(e) = result {
            out.problems.push((f, e));
        }
    }
    dedupe_keys(&mut out.playlists);
    out
}

fn collect(p: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(rd) = fs::read_dir(p) else { return };
        for e in rd.flatten() {
            collect(&e.path(), depth + 1, out);
        }
    } else if kind(p).is_some() {
        out.push(p.to_path_buf());
    }
}

/// The export files this module reads.
enum Kind {
    Zip,
    Csv,
    Library,
    Playlists,
}

fn kind(p: &Path) -> Option<Kind> {
    let lower = |s: Option<&std::ffi::OsStr>| {
        s.map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let (stem, ext) = (lower(p.file_stem()), lower(p.extension()));
    match ext.as_str() {
        "csv" => Some(Kind::Csv),
        "zip" if stem.contains("spotify") => Some(Kind::Zip),
        "json" if stem.starts_with("yourlibrary") => Some(Kind::Library),
        "json" if stem.starts_with("playlist") => Some(Kind::Playlists),
        _ => None,
    }
}

fn read_json(p: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(p).map_err(|e| e.to_string())?;
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("not valid JSON: {e}"))
}

fn str_field(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// `Playlist1.json`: `{"playlists": [{"name", "description", "items": [{"track": {...}}]}]}`.
fn parse_playlists(v: &Value) -> Vec<SpotifyPlaylist> {
    let Some(list) = v.get("playlists").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|p| {
            let name = str_field(p, &["name"])?;
            let mut pl = SpotifyPlaylist {
                source_key: format!("spotify:{name}"),
                name,
                description: str_field(p, &["description"]),
                ..SpotifyPlaylist::default()
            };
            for item in p
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(t) = item
                    .get("track")
                    .filter(|t| t.is_object())
                    .and_then(track_from_export)
                {
                    pl.tracks.push(t);
                } else if let Some(t) = item
                    .get("localTrack")
                    .and_then(|l| str_field(l, &["uri"]))
                    .and_then(|u| local_track(&u))
                {
                    pl.tracks.push(t);
                } else {
                    pl.skipped += 1;
                }
            }
            Some(pl)
        })
        .collect()
}

fn track_from_export(t: &Value) -> Option<SpotifyTrack> {
    Some(SpotifyTrack {
        title: str_field(t, &["trackName", "track"])?,
        artist: str_field(t, &["artistName", "artist"]).unwrap_or_default(),
        album: str_field(t, &["albumName", "album"]),
        duration_ms: None,
        isrc: None,
        uri: str_field(t, &["trackUri", "uri"]),
    })
}

/// `YourLibrary.json`: `{"tracks": [{"artist", "album", "track", "uri"}]}`.
fn parse_library(v: &Value) -> Vec<SpotifyTrack> {
    v.get("tracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(track_from_export)
        .collect()
}

/// `spotify:local:Artist:Album:Title:Seconds`, URL-encoded with `+` for spaces.
fn local_track(uri: &str) -> Option<SpotifyTrack> {
    let rest = uri.strip_prefix("spotify:local:")?;
    let parts: Vec<String> = rest.split(':').map(url_decode).collect();
    let [artist, album, title, secs] = parts.as_slice() else {
        return None;
    };
    (!title.is_empty()).then(|| SpotifyTrack {
        title: title.clone(),
        artist: artist.clone(),
        album: (!album.is_empty()).then(|| album.clone()),
        duration_ms: secs.parse::<u64>().ok().map(|s| s * 1000),
        isrc: None,
        uri: Some(uri.to_owned()),
    })
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(s.get(i + 1..i + 3).unwrap_or(""), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- CSV --------------------------------------------------------------------

fn read_csv_playlist(p: &Path) -> Result<SpotifyPlaylist, String> {
    let text = fs::read_to_string(p).map_err(|e| e.to_string())?;
    let rows = parse_csv(text.trim_start_matches('\u{feff}'));
    let mut rows = rows.into_iter();
    let header: Vec<String> = rows
        .next()
        .ok_or("the file is empty")?
        .into_iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let col = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
    let title = col(&["track name", "name", "title", "song", "track"])
        .ok_or("no track name column; is this a playlist export?")?;
    let artist = col(&[
        "artist name(s)",
        "artist name",
        "artist names",
        "artist",
        "artists",
    ])
    .ok_or("no artist column; is this a playlist export?")?;
    let album = col(&["album name", "album"]);
    let isrc = col(&["isrc"]);
    let uri = col(&["track uri", "spotify uri", "uri"]);
    let duration = header
        .iter()
        .position(|h| h.contains("duration") && h.contains("ms"));

    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = stem.replace('_', " ").trim().to_owned();
    let mut pl = SpotifyPlaylist {
        source_key: format!("spotify-csv:{stem}"),
        name: if name.is_empty() {
            "Imported playlist".into()
        } else {
            name
        },
        ..SpotifyPlaylist::default()
    };
    for r in rows {
        let get = |i: Option<usize>| {
            i.and_then(|i| r.get(i))
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let Some(t) = get(Some(title)) else {
            if r.iter().any(|c| !c.trim().is_empty()) {
                pl.skipped += 1;
            }
            continue;
        };
        pl.tracks.push(SpotifyTrack {
            title: t,
            artist: get(Some(artist)).unwrap_or_default(),
            album: get(album),
            duration_ms: get(duration).and_then(|d| d.parse().ok()),
            isrc: get(isrc),
            uri: get(uri),
        });
    }
    Ok(pl)
}

/// RFC 4180 CSV: quoted fields may hold commas, quotes (`""`) and newlines.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Spotify allows two playlists with the same name; keep their keys distinct
/// (and stable, since the export lists them in a fixed order).
fn dedupe_keys(playlists: &mut [SpotifyPlaylist]) {
    let mut seen = std::collections::HashMap::<String, u32>::new();
    for p in playlists {
        let n = seen.entry(p.source_key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            p.source_key = format!("{}#{n}", p.source_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_account_data_playlist_format() {
        let v: Value = serde_json::from_str(
            r#"{"playlists":[{"name":"Road Trip","lastModifiedDate":"2024-05-01","description":"loud",
              "items":[
                {"track":{"trackName":"Dreamer","artistName":"Ozzy Osbourne","albumName":"Down to Earth","trackUri":"spotify:track:abc"},"episode":null,"localTrack":null,"addedDate":"2024-01-01"},
                {"track":null,"episode":{"episodeName":"Ep 1","showName":"Pod","episodeUri":"spotify:episode:x"},"localTrack":null},
                {"track":null,"episode":null,"localTrack":{"uri":"spotify:local:Some+Band:An+Album:Caf%C3%A9+Song:215"}}
              ],"numberOfFollowers":0}]}"#,
        )
        .unwrap();
        let pls = parse_playlists(&v);
        assert_eq!(pls.len(), 1);
        let p = &pls[0];
        assert_eq!(
            (p.name.as_str(), p.description.as_deref(), p.skipped),
            ("Road Trip", Some("loud"), 1)
        );
        assert_eq!(p.tracks[0].title, "Dreamer");
        assert_eq!(p.tracks[0].uri.as_deref(), Some("spotify:track:abc"));
        assert_eq!(
            p.tracks[1],
            SpotifyTrack {
                title: "Café Song".into(),
                artist: "Some Band".into(),
                album: Some("An Album".into()),
                duration_ms: Some(215_000),
                isrc: None,
                uri: Some("spotify:local:Some+Band:An+Album:Caf%C3%A9+Song:215".into()),
            }
        );
    }

    #[test]
    fn reads_liked_songs() {
        let v: Value = serde_json::from_str(
            r#"{"tracks":[{"artist":"Gorillaz","album":"Demon Days","track":"Feel Good Inc.","uri":"spotify:track:1"}],"albums":[]}"#,
        )
        .unwrap();
        let liked = parse_library(&v);
        assert_eq!(liked.len(), 1);
        assert_eq!(
            (liked[0].title.as_str(), liked[0].artist.as_str()),
            ("Feel Good Inc.", "Gorillaz")
        );
    }

    #[test]
    fn reads_csv_exports_with_quotes_and_commas() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("road_trip.csv");
        fs::write(
            &f,
            "\u{feff}\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Track Duration (ms)\",\"ISRC\"\r\n\
             \"spotify:track:1\",\"Hello, \"\"World\"\"\",\"A,B\",\"LP\",\"201000\",\"USAB12345678\"\r\n\
             \"spotify:track:2\",\"Second\",\"C\",\"\",\"\",\"\"\r\n",
        )
        .unwrap();
        let p = read_csv_playlist(&f).unwrap();
        assert_eq!(p.name, "road trip");
        assert_eq!(p.tracks.len(), 2);
        assert_eq!(p.tracks[0].title, "Hello, \"World\"");
        assert_eq!(p.tracks[0].artist, "A,B");
        assert_eq!(p.tracks[0].duration_ms, Some(201_000));
        assert_eq!(p.tracks[0].isrc.as_deref(), Some("USAB12345678"));
        assert_eq!(p.tracks[1].album, None);
    }

    #[test]
    fn finds_files_in_a_nested_download_folder_and_reports_bad_ones() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("Spotify Account Data");
        fs::create_dir(&inner).unwrap();
        fs::write(
            inner.join("Playlist1.json"),
            r#"{"playlists":[{"name":"A","items":[]},{"name":"A","items":[]}]}"#,
        )
        .unwrap();
        fs::write(inner.join("YourLibrary.json"), r#"{"tracks":[]}"#).unwrap();
        fs::write(inner.join("Playlist2.json"), "{not json").unwrap();
        fs::write(inner.join("StreamingHistory0.json"), "[]").unwrap();
        let e = read_export(&[dir.path().to_path_buf()]);
        let keys: Vec<_> = e.playlists.iter().map(|p| p.source_key.as_str()).collect();
        assert_eq!(keys, ["spotify:A", "spotify:A#2"]);
        assert_eq!(e.liked, Some(vec![]));
        assert_eq!(e.problems.len(), 1);
        assert!(e.problems[0].0.ends_with("Playlist2.json"));
    }

    #[test]
    fn a_zip_is_reported_not_ignored() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("my_spotify_data.zip"), b"PK").unwrap();
        let e = read_export(&[dir.path().to_path_buf()]);
        assert!(e.problems[0].1.contains("unzip"));
    }
}
