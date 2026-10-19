// SPDX-License-Identifier: GPL-3.0-only
//! End to end: a Spotify data download imported into a real (in-memory) database.

use std::fs;

use stillwave_db::{Db, EntryRow, FileFacts, TrackImport};
use stillwave_library::{import_spotify, resolve_placeholders};

fn track(path: &str, title: &str, artist: Option<&str>) -> TrackImport {
    TrackImport {
        title: title.into(),
        artist: artist.map(String::from),
        album_artist: None,
        album: None,
        year: None,
        track_number: None,
        disc_number: None,
        isrc: None,
        file: FileFacts {
            path: path.into(),
            size_bytes: 1000,
            modified_unix_ms: 5,
            container: "flac".into(),
            codec: "flac".into(),
            sample_rate_hz: 44_100,
            bit_depth: Some(16),
            channels: 2,
            duration_ms: 180_000,
        },
    }
}

const PLAYLISTS: &str = r#"{"playlists":[
  {"name":"Road Trip","description":null,"items":[
    {"track":{"trackName":"Dreamer - 2011 Remaster","artistName":"Ozzy Osbourne","albumName":"Down to Earth","trackUri":"spotify:track:1"}},
    {"track":{"trackName":"Not Owned","artistName":"Nobody","albumName":"Nothing","trackUri":"spotify:track:2"}},
    {"track":null,"episode":{"episodeName":"Pod"}},
    {"track":{"trackName":"San Quentin (Instrumental)","artistName":"Nickelback","albumName":"Feed the Machine","trackUri":"spotify:track:3"}}
  ]},
  {"name":"Chill","items":[
    {"track":{"trackName":"Not Owned","artistName":"Nobody","albumName":"Nothing","trackUri":"spotify:track:2"}}
  ]}
]}"#;

const LIBRARY: &str = r#"{"tracks":[
  {"artist":"Ozzy Osbourne","album":"Down to Earth","track":"Dreamer","uri":"spotify:track:1"},
  {"artist":"Nobody","album":"Nothing","track":"Not Owned","uri":"spotify:track:2"}
]}"#;

fn titles(rows: &[EntryRow]) -> Vec<(String, bool)> {
    rows.iter()
        .map(|r| match r {
            EntryRow::Track(t) => (t.title.clone(), true),
            EntryRow::Missing { title, .. } => (title.clone(), false),
        })
        .collect()
}

#[test]
fn imports_matches_reimports_and_fills_in_later() {
    let mut db = Db::open_in_memory().unwrap();
    db.import_tracks(&[
        track(
            "/m/dreamer.flac",
            "Dreamer",
            Some("Ozzy Osbourne feat. Mike McCready"),
        ),
        track(
            "/m/sq.flac",
            "12 - Nickelback - San Quentin (Instrumental)",
            None,
        ),
    ])
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("Spotify Account Data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("Playlist1.json"), PLAYLISTS).unwrap();
    fs::write(data.join("YourLibrary.json"), LIBRARY).unwrap();

    let s = import_spotify(&mut db, &[dir.path().to_path_buf()]).unwrap();
    let got: Vec<_> = s
        .playlists
        .iter()
        .map(|p| (p.name.as_str(), p.total, p.matched))
        .collect();
    assert_eq!(got, [("Road Trip", 3, 2), ("Chill", 1, 0)]);
    assert_eq!(s.skipped, 1);
    assert_eq!(s.liked, Some((2, 1)));

    let road = &s.playlists[0].id;
    assert_eq!(
        titles(&db.playlist_entries(road).unwrap()),
        [
            ("Dreamer".to_owned(), true),
            ("Not Owned".to_owned(), false),
            (
                "12 - Nickelback - San Quentin (Instrumental)".to_owned(),
                true
            ),
        ]
    );
    assert_eq!(
        titles(&db.liked_entries().unwrap()),
        [
            ("Dreamer".to_owned(), true),
            ("Not Owned".to_owned(), false)
        ],
        "liked songs keep the export's order"
    );
    // One song not owned, in two playlists and liked: one placeholder.
    assert_eq!(db.open_placeholders().unwrap().len(), 1);

    // Importing again updates instead of duplicating.
    let changes = db.change_count().unwrap();
    let again = import_spotify(&mut db, &[dir.path().to_path_buf()]).unwrap();
    assert_eq!(again.playlists[0].id, *road);
    assert_eq!(db.playlists().unwrap().len(), 2);
    assert_eq!(db.open_placeholders().unwrap().len(), 1);
    assert_eq!(db.liked_entries().unwrap().len(), 2);
    assert!(
        db.change_count().unwrap() > changes,
        "re-import is logged for sync"
    );

    // The user adds the missing song and rescans: the placeholder resolves.
    db.import_tracks(&[track("/m/owned-now.flac", "Not Owned", Some("Nobody"))])
        .unwrap();
    assert_eq!(resolve_placeholders(&mut db).unwrap(), 1);
    assert!(titles(&db.playlist_entries(road).unwrap())
        .iter()
        .all(|(_, owned)| *owned));
    let chill = db
        .playlists()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Chill")
        .unwrap();
    assert_eq!((chill.entry_count, chill.owned_count), (1, 1));
    assert_eq!(db.like_counts().unwrap(), (2, 2));
}

#[test]
fn a_folder_without_spotify_files_is_a_clear_error() {
    let mut db = Db::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), "hi").unwrap();
    let err = import_spotify(&mut db, &[dir.path().to_path_buf()]).unwrap_err();
    assert!(err.to_string().contains("Playlist1.json"), "{err}");
    assert!(db.playlists().unwrap().is_empty());
}
