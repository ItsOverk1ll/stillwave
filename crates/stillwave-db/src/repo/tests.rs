// SPDX-License-Identifier: GPL-3.0-only
use std::collections::BTreeSet;

use rusqlite::Connection;
use stillwave_core::{
    Artist, ArtistId, EntryTarget, ExternalIds, Like, LikeId, MediaItem, MediaItemId, MediaKind,
    Placeholder, PlaceholderId, Playlist, PlaylistEntry, PlaylistEntryId, PlaylistId,
};

use super::*;

fn item(title: &str, artist: Option<ArtistId>) -> MediaItem {
    MediaItem {
        id: MediaItemId::new(),
        kind: MediaKind::Track,
        title: title.into(),
        artist_id: artist,
        album_id: None,
        track_number: Some(1),
        disc_number: None,
        duration_ms: Some(200_000),
        isrc: None,
        external_ids: ExternalIds::new(),
    }
}

fn artist(name: &str) -> Artist {
    Artist {
        id: ArtistId::new(),
        name: name.into(),
        sort_name: None,
        external_ids: ExternalIds::new(),
    }
}

#[test]
fn migrations_apply_to_an_empty_database() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), crate::latest_version());
    assert!(db.integrity_ok().unwrap());
}

#[test]
fn migrated_schema_matches_expected_objects() {
    let db = Db::open_in_memory().unwrap();
    let names: BTreeSet<String> = db
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' AND name NOT LIKE 'search_index_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let expected: BTreeSet<String> = [
        "meta",
        "artists",
        "albums",
        "media_items",
        "media_files",
        "placeholders",
        "playlists",
        "playlist_entries",
        "plays",
        "likes",
        "change_log",
        "search_index",
        "media_items_ai",
        "media_items_ad",
        "media_items_au",
        "artists_au",
        "albums_au",
        "idx_media_items_artist",
        "idx_media_items_album",
        "idx_media_items_isrc",
        "idx_media_files_item",
        "idx_placeholders_isrc",
        "idx_playlist_entries_order",
        "idx_plays_item",
        "idx_plays_time",
        "idx_likes_item",
        "idx_likes_placeholder",
        "idx_change_log_hlc",
        "idx_change_log_entity",
        "idx_artists_name",
        "idx_albums_title",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let extra: Vec<_> = names.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&names).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "extra {extra:?}, missing {missing:?}"
    );
}

#[test]
fn rejects_a_database_from_the_future() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    assert!(matches!(
        crate::migrate(&mut conn),
        Err(DbError::NewerSchema { found: 999, .. })
    ));
}

#[test]
fn migrating_twice_is_a_no_op() {
    let mut db = Db::open_in_memory().unwrap();
    crate::migrate(&mut db.conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), crate::latest_version());
}

#[test]
fn media_item_round_trips() {
    let mut db = Db::open_in_memory().unwrap();
    let a = artist("Boards of Canada");
    db.insert_artist(&a).unwrap();
    let mut i = item("Roygbiv", Some(a.id));
    i.external_ids
        .insert("spotify".into(), "spotify:track:xyz".into());
    db.insert_media_item(&i).unwrap();
    assert_eq!(db.get_media_item(i.id).unwrap(), Some(i));
    assert_eq!(db.get_artist(a.id).unwrap(), Some(a));
    assert_eq!(db.get_media_item(MediaItemId::new()).unwrap(), None);
}

#[test]
fn every_write_appends_to_the_change_log() {
    let mut db = Db::open_in_memory().unwrap();
    db.insert_media_items(&[item("a", None), item("b", None), item("c", None)])
        .unwrap();
    let i = item("d", None);
    db.insert_media_item(&i).unwrap();
    db.rename_media_item(i.id, "d2").unwrap();
    db.delete_media_item(i.id).unwrap();
    assert_eq!(db.change_count().unwrap(), 6);
    // HLCs are strictly increasing in write order.
    let hlcs: Vec<String> = db
        .conn
        .prepare("SELECT hlc FROM change_log ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(hlcs.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn failed_write_rolls_back_data_and_change_log() {
    let mut db = Db::open_in_memory().unwrap();
    let i = item("doomed", None);
    let r: Result<()> = db.write(|tx, log| {
        tx.execute(
            "INSERT INTO media_items (id, kind, title) VALUES (?1, 'track', 'doomed')",
            params![i.id.as_bytes().as_slice()],
        )?;
        log.record(tx, "media_item", i.id.as_bytes(), "insert", &["title"])?;
        Err(DbError::Corrupt("simulated failure mid-transaction".into()))
    });
    assert!(r.is_err());
    assert_eq!(db.media_item_count().unwrap(), 0);
    assert_eq!(db.change_count().unwrap(), 0);
    assert!(db.search("doomed", 10).unwrap().is_empty());
}

#[test]
fn uncommitted_transaction_leaves_database_clean_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.db");
    {
        let mut db = Db::open(&path).unwrap();
        db.insert_media_item(&item("committed", None)).unwrap();
    }
    {
        // Simulate a process dying mid-write: a transaction is started and the
        // connection is dropped without commit.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "BEGIN; INSERT INTO media_items (id, kind, title) VALUES (randomblob(16), 'track', 'half-written');",
        )
        .unwrap();
        drop(conn);
    }
    let db = Db::open(&path).unwrap();
    assert!(db.integrity_ok().unwrap());
    assert_eq!(db.media_item_count().unwrap(), 1);
    assert_eq!(db.search("committed", 5).unwrap().len(), 1);
    assert!(db.search("half", 5).unwrap().is_empty());
}

#[test]
fn device_id_and_clock_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.db");
    let (device, first) = {
        let mut db = Db::open(&path).unwrap();
        db.insert_media_item(&item("x", None)).unwrap();
        (db.device_id(), db.clock.last())
    };
    let mut db = Db::open(&path).unwrap();
    assert_eq!(db.device_id(), device);
    db.insert_media_item(&item("y", None)).unwrap();
    assert!(db.clock.last() > first);
}

#[test]
fn search_is_prefix_multiword_and_diacritic_insensitive() {
    let mut db = Db::open_in_memory().unwrap();
    let a = artist("Sigur Rós");
    db.insert_artist(&a).unwrap();
    db.insert_media_items(&[
        item("Hoppípolla", Some(a.id)),
        item("Svefn-g-englar", Some(a.id)),
        item("Other", None),
    ])
    .unwrap();
    assert_eq!(db.search("hopp", 10).unwrap().len(), 1);
    assert_eq!(db.search("sigur ros", 10).unwrap().len(), 2); // artist match, no accent
    assert_eq!(db.search("svefn eng", 10).unwrap().len(), 1);
    assert!(db.search("   ", 10).unwrap().is_empty());
    assert!(db.search("\"*(", 10).unwrap().is_empty()); // hostile input is safe
}

#[test]
fn search_index_follows_renames_and_deletes() {
    let mut db = Db::open_in_memory().unwrap();
    let a = artist("Old Name");
    db.insert_artist(&a).unwrap();
    let i = item("Song", Some(a.id));
    db.insert_media_item(&i).unwrap();
    db.rename_media_item(i.id, "Renamed").unwrap();
    assert_eq!(db.search("renamed", 5).unwrap().len(), 1);
    assert!(db.search("song", 5).unwrap().is_empty());

    db.conn
        .execute(
            "UPDATE artists SET name = 'New Name' WHERE id = ?1",
            params![a.id.as_bytes().as_slice()],
        )
        .unwrap();
    assert_eq!(db.search("new name", 5).unwrap().len(), 1);
    assert!(db.search("old", 5).unwrap().is_empty());

    db.delete_media_item(i.id).unwrap();
    assert!(db.search("renamed", 5).unwrap().is_empty());
}

#[test]
fn playlist_entries_can_point_at_items_or_placeholders() {
    let mut db = Db::open_in_memory().unwrap();
    let pl = Playlist {
        id: PlaylistId::new(),
        name: "Imported".into(),
        description: None,
    };
    db.insert_playlist(&pl).unwrap();
    let i = item("Owned", None);
    db.insert_media_item(&i).unwrap();
    let ph = Placeholder {
        id: PlaceholderId::new(),
        title: "Not owned".into(),
        artist: "Somebody".into(),
        album: None,
        duration_ms: None,
        isrc: Some("USRC17607839".into()),
        external_ids: ExternalIds::from([("spotify".to_string(), "spotify:track:1".to_string())]),
        resolved_to: None,
    };
    db.insert_placeholder(&ph).unwrap();
    for (pos, target) in [
        ("a", EntryTarget::Placeholder(ph.id)),
        ("b", EntryTarget::Item(i.id)),
    ] {
        db.insert_playlist_entry(&PlaylistEntry {
            id: PlaylistEntryId::new(),
            playlist_id: pl.id,
            target,
            position: pos.into(),
        })
        .unwrap();
    }
    let entries = db.list_playlist_entries(pl.id).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].target, EntryTarget::Placeholder(ph.id));
    assert_eq!(entries[1].target, EntryTarget::Item(i.id));
}

#[test]
fn entry_with_no_target_is_rejected_by_the_database() {
    let db = Db::open_in_memory().unwrap();
    db.conn
        .execute("INSERT INTO playlists (id, name) VALUES (x'01', 'p')", [])
        .unwrap();
    let err = db.conn.execute(
        "INSERT INTO playlist_entries (id, playlist_id, position) VALUES (x'02', x'01', 'a')",
        [],
    );
    assert!(err.is_err());
}

#[test]
fn foreign_keys_are_enforced_and_deleting_an_item_cascades() {
    let mut db = Db::open_in_memory().unwrap();
    let orphan = db.insert_like(&Like {
        id: LikeId::new(),
        target: EntryTarget::Item(MediaItemId::new()),
        liked_at: db.clock.clone().tick(),
    });
    assert!(orphan.is_err());

    let i = item("liked", None);
    db.insert_media_item(&i).unwrap();
    let liked_at = db.clock.clone().tick();
    db.insert_like(&Like {
        id: LikeId::new(),
        target: EntryTarget::Item(i.id),
        liked_at,
    })
    .unwrap();
    assert!(db.is_liked(EntryTarget::Item(i.id)).unwrap());
    db.delete_media_item(i.id).unwrap();
    assert!(!db.is_liked(EntryTarget::Item(i.id)).unwrap());
}

fn track(path: &str, title: &str, artist: &str, album: &str) -> TrackImport {
    TrackImport {
        title: title.into(),
        artist: Some(artist.into()),
        album_artist: None,
        album: Some(album.into()),
        year: Some(2001),
        track_number: Some(1),
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

#[test]
fn import_creates_shared_artists_and_albums_case_insensitively() {
    let mut db = Db::open_in_memory().unwrap();
    let counts = db
        .import_tracks(&[
            track("/m/a.flac", "One", "Daft Punk", "Discovery"),
            track("/m/b.flac", "Two", "daft punk", "DISCOVERY"),
        ])
        .unwrap();
    assert_eq!((counts.added, counts.updated), (2, 0));
    let n = |sql: &str| db.conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(n("SELECT COUNT(*) FROM artists"), 1);
    assert_eq!(n("SELECT COUNT(*) FROM albums"), 1);
    assert_eq!(db.media_item_count().unwrap(), 2);
    assert_eq!(db.file_count().unwrap(), 2);
    assert_eq!(db.search("discovery", 10).unwrap().len(), 2);
    assert_eq!(db.search("daft", 10).unwrap().len(), 2);
}

#[test]
fn reimporting_a_path_updates_in_place() {
    let mut db = Db::open_in_memory().unwrap();
    db.import_tracks(&[track("/m/a.flac", "Old", "Band", "Album")])
        .unwrap();
    let before = db.list_media_items(10, 0).unwrap()[0].id;
    let mut changed = track("/m/a.flac", "New title", "Band", "Album");
    changed.file.size_bytes = 2000;
    let counts = db.import_tracks(&[changed]).unwrap();
    assert_eq!((counts.added, counts.updated), (0, 1));
    let items = db.list_media_items(10, 0).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, before, "item identity survives a retag");
    assert_eq!(items[0].title, "New title");
    assert_eq!(db.stored_files().unwrap()["/m/a.flac"].size_bytes, 2000);
    assert_eq!(db.search("old", 5).unwrap().len(), 0);
}

#[test]
fn removing_a_file_keeps_the_item_and_its_playlist_entries() {
    let mut db = Db::open_in_memory().unwrap();
    db.import_tracks(&[track("/m/a.flac", "Keep me", "Band", "Album")])
        .unwrap();
    let item = db.list_media_items(1, 0).unwrap()[0].id;
    let pl = Playlist {
        id: PlaylistId::new(),
        name: "p".into(),
        description: None,
    };
    db.insert_playlist(&pl).unwrap();
    db.insert_playlist_entry(&PlaylistEntry {
        id: PlaylistEntryId::new(),
        playlist_id: pl.id,
        target: EntryTarget::Item(item),
        position: "a".into(),
    })
    .unwrap();
    assert_eq!(
        db.remove_files(&["/m/a.flac".into(), "/m/none.flac".into()])
            .unwrap(),
        1
    );
    assert_eq!(db.file_count().unwrap(), 0);
    assert_eq!(db.list_playlist_entries(pl.id).unwrap().len(), 1);
    assert!(db.get_media_item(item).unwrap().is_some());
}

#[test]
fn failed_import_batch_rolls_back_completely() {
    let mut db = Db::open_in_memory().unwrap();
    db.import_tracks(&[track("/m/a.flac", "A", "Band", "Album")])
        .unwrap();
    let changes = db.change_count().unwrap();
    // Second track has a title that violates NOT NULL? Use a poisoned device row instead:
    // make the batch fail on its second element by dropping the table it needs.
    db.conn.execute_batch("DROP TABLE media_files").unwrap();
    let r = db.import_tracks(&[track("/m/b.flac", "B", "Other", "Other Album")]);
    assert!(r.is_err());
    assert_eq!(db.change_count().unwrap(), changes);
    assert_eq!(db.media_item_count().unwrap(), 1);
    let artists: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM artists", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        artists, 1,
        "artist created earlier in the failed batch was rolled back"
    );
}
