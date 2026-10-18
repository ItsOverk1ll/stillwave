// SPDX-License-Identifier: GPL-3.0-only
//! Scanner tests on small generated WAV files (tags written with Lofty into
//! temp copies; nothing here touches a real library).

use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt, TagType};
use stillwave_db::Db;
use stillwave_library::{read_track, scan, ScanError, ScanOptions, ScanProgress};

/// 0.1 s of stereo 16-bit 44.1 kHz silence.
fn write_wav(path: &Path) {
    let frames: u32 = 4410;
    let data_len = frames * 4;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&44_100u32.to_le_bytes());
    b.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.extend(std::iter::repeat_n(0u8, data_len as usize));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b).unwrap();
}

fn tag_wav(path: &Path, title: &str, artist: &str, album: &str, track: u32) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::TrackTitle, title.into());
    tag.insert_text(ItemKey::TrackArtist, artist.into());
    tag.insert_text(ItemKey::AlbumTitle, album.into());
    tag.set_track(track);
    tag.set_track_total(12);
    tag.insert_text(ItemKey::RecordingDate, "1999-05-04".into());
    tag.insert_text(ItemKey::Isrc, "USRC17607839".into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn opts() -> ScanOptions {
    ScanOptions::default()
}

#[test]
fn reads_tags_and_audio_properties() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    write_wav(&p);
    tag_wav(&p, "Song", "The Band", "The Album", 3);
    let t = read_track(&p).unwrap();
    assert_eq!(t.title, "Song");
    assert_eq!(t.artist.as_deref(), Some("The Band"));
    assert_eq!(t.album.as_deref(), Some("The Album"));
    assert_eq!(t.track_number, Some(3));
    assert_eq!(t.year, Some(1999));
    assert_eq!(t.isrc.as_deref(), Some("USRC17607839"));
    assert_eq!(
        (t.file.sample_rate_hz, t.file.channels, t.file.bit_depth),
        (44_100, 2, Some(16))
    );
    assert_eq!(
        (t.file.container.as_str(), t.file.codec.as_str()),
        ("wav", "pcm")
    );
    assert!(
        (99..=101).contains(&t.file.duration_ms),
        "{}",
        t.file.duration_ms
    );
}

#[test]
fn untagged_file_takes_its_title_from_the_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("Lonely Track.wav");
    write_wav(&p);
    let t = read_track(&p).unwrap();
    assert_eq!(t.title, "Lonely Track");
    assert_eq!(t.artist, None);
}

#[test]
fn scan_adds_files_skips_junk_and_reports_unreadable_ones() {
    let dir = tempfile::tempdir().unwrap();
    for (i, sub) in ["", "disc1", "disc1/deep"].iter().enumerate() {
        let p = dir.path().join(sub).join(format!("t{i}.wav"));
        write_wav(&p);
        tag_wav(&p, &format!("Title {i}"), "Artist", "Album", 1);
    }
    fs::write(dir.path().join("notes.txt"), "not audio").unwrap();
    fs::write(dir.path().join("cover.jpg"), [0xff, 0xd8]).unwrap();
    fs::write(
        dir.path().join("broken.wav"),
        b"this is not a wav file at all",
    )
    .unwrap();

    let mut db = Db::open_in_memory().unwrap();
    let mut events = Vec::new();
    let r = scan(&mut db, dir.path(), &opts(), |p| events.push(p)).unwrap();
    assert_eq!((r.found, r.added, r.updated, r.unchanged), (4, 3, 0, 0));
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].0.ends_with("broken.wav"));
    assert_eq!(db.media_item_count().unwrap(), 3);
    assert_eq!(db.search("title", 10).unwrap().len(), 3);
    assert!(events
        .iter()
        .any(|e| matches!(e, ScanProgress::Importing { total: 4, .. })));
}

#[test]
fn rescan_only_touches_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
    for p in [&a, &b] {
        write_wav(p);
        tag_wav(p, "Old", "Artist", "Album", 1);
    }
    let mut db = Db::open_in_memory().unwrap();
    scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();

    let again = scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();
    assert_eq!((again.added, again.updated, again.unchanged), (0, 0, 2));

    tag_wav(&a, "A much longer new title", "Artist", "Album", 1);
    let after = scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();
    assert_eq!((after.added, after.updated, after.unchanged), (0, 1, 1));
    assert_eq!(
        db.media_item_count().unwrap(),
        2,
        "retagging must not duplicate"
    );
    assert_eq!(db.search("longer", 5).unwrap().len(), 1);

    let forced = scan(
        &mut db,
        dir.path(),
        &ScanOptions {
            force: true,
            ..opts()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!((forced.updated, forced.unchanged), (2, 0));
}

#[test]
fn deleted_files_are_forgotten_only_when_asked_and_only_under_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let (a, b, c) = (
        dir.path().join("a.wav"),
        dir.path().join("b.wav"),
        other.path().join("c.wav"),
    );
    for p in [&a, &b, &c] {
        write_wav(p);
    }
    let mut db = Db::open_in_memory().unwrap();
    scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();
    scan(&mut db, other.path(), &opts(), |_| {}).unwrap();
    assert_eq!(db.file_count().unwrap(), 3);

    fs::remove_file(&a).unwrap();
    let keep = scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();
    assert_eq!(keep.removed, 0);
    assert_eq!(db.file_count().unwrap(), 3);

    let tidy = ScanOptions {
        remove_missing: true,
        ..opts()
    };
    let r = scan(&mut db, dir.path(), &tidy, |_| {}).unwrap();
    assert_eq!(r.removed, 1);
    assert_eq!(
        db.file_count().unwrap(),
        2,
        "files under other roots are untouched"
    );
    assert_eq!(
        db.media_item_count().unwrap(),
        3,
        "items survive for playlists"
    );
}

#[test]
fn a_cancelled_scan_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(&dir.path().join("a.wav"));
    let mut db = Db::open_in_memory().unwrap();
    let o = opts();
    o.cancel.store(true, Ordering::Relaxed);
    let r = scan(&mut db, dir.path(), &o, |_| {}).unwrap();
    assert!(r.cancelled);
    assert_eq!(db.media_item_count().unwrap(), 0);
}

#[test]
fn missing_root_is_an_error() {
    let mut db = Db::open_in_memory().unwrap();
    let r = scan(
        &mut db,
        Path::new("Z:/definitely/not/here"),
        &opts(),
        |_| {},
    );
    assert!(matches!(r, Err(ScanError::BadRoot(_))));
}

#[test]
fn scanning_never_modifies_audio_files() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    write_wav(&p);
    tag_wav(&p, "T", "A", "B", 1);
    let before = fs::read(&p).unwrap();
    let mtime = fs::metadata(&p).unwrap().modified().unwrap();
    let mut db = Db::open_in_memory().unwrap();
    scan(&mut db, dir.path(), &opts(), |_| {}).unwrap();
    assert_eq!(fs::read(&p).unwrap(), before);
    assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), mtime);
    // keep the unused import honest
    let _ = lofty::read_from_path(&p).unwrap().tags().len();
}
