// SPDX-License-Identifier: GPL-3.0-only
//! Library screen queries on 50,000 imported tracks (target: page open < 100 ms).
//! Run with `cargo bench -p stillwave-db --bench library`.

use criterion::{criterion_group, criterion_main, Criterion};
use stillwave_db::{Db, FileFacts, TrackImport};

const WORDS: &[&str] = &[
    "night", "river", "golden", "echo", "paper", "shadow", "ocean", "neon", "velvet", "static",
    "winter", "signal", "garden", "mirror", "thunder", "silver", "dream", "highway", "ember",
    "tide",
];

fn seed(db: &mut Db) {
    let tracks: Vec<TrackImport> = (0..50_000usize)
        .map(|n| TrackImport {
            title: format!("{} {} {n}", WORDS[n % 20], WORDS[(n * 7 + 3) % 20]),
            artist: Some(format!(
                "{} {} {}",
                WORDS[n % 20],
                WORDS[(n / 20) % 20],
                n % 2000
            )),
            album_artist: None,
            album: Some(format!("{} album {}", WORDS[(n / 7) % 20], n % 5000)),
            year: Some(2000),
            track_number: Some(u32::try_from(n % 14).unwrap_or(0) + 1),
            disc_number: Some(1),
            isrc: None,
            file: FileFacts {
                path: format!("/music/{n}.flac"),
                size_bytes: 30_000_000,
                modified_unix_ms: 1,
                container: "flac".into(),
                codec: "flac".into(),
                sample_rate_hz: 44_100,
                bit_depth: Some(16),
                channels: 2,
                duration_ms: 200_000,
            },
        })
        .collect();
    let start = std::time::Instant::now();
    for chunk in tracks.chunks(250) {
        db.import_tracks(chunk).unwrap();
    }
    eprintln!("imported 50,000 tracks in {:?}", start.elapsed());
}

fn bench(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Db::open(dir.path().join("bench.db")).unwrap();
    seed(&mut db);
    let mut g = c.benchmark_group("library_50k");
    g.bench_function("page_100_at_0", |b| {
        b.iter(|| db.library_rows("", 100, 0).unwrap());
    });
    g.bench_function("page_100_at_25000", |b| {
        b.iter(|| db.library_rows("", 100, 25_000).unwrap());
    });
    g.bench_function("page_100_at_49900", |b| {
        b.iter(|| db.library_rows("", 100, 49_900).unwrap());
    });
    g.bench_function("search_golden_page", |b| {
        b.iter(|| db.library_rows("golden", 100, 0).unwrap());
    });
    g.bench_function("search_golden_count", |b| {
        b.iter(|| db.library_count("golden").unwrap());
    });
    let albums = db.album_count().unwrap();
    let artists = db.artist_count().unwrap();
    eprintln!("albums: {albums}, artists: {artists}");
    let deep_album = u32::try_from(albums.saturating_sub(100)).unwrap_or(0);
    let deep_artist = u32::try_from(artists.saturating_sub(100)).unwrap_or(0);
    g.bench_function("albums_page_100_at_0", |b| {
        b.iter(|| db.albums_page(100, 0).unwrap());
    });
    g.bench_function("albums_page_100_at_end", |b| {
        b.iter(|| db.albums_page(100, deep_album).unwrap());
    });
    g.bench_function("artists_page_100_at_end", |b| {
        b.iter(|| db.artists_page(100, deep_artist).unwrap());
    });
    let some_album = db.albums_page(1, 0).unwrap().remove(0).id;
    let some_artist = db.artists_page(1, 0).unwrap().remove(0).id;
    g.bench_function("album_tracks", |b| {
        b.iter(|| db.album_tracks(&some_album).unwrap());
    });
    g.bench_function("artist_albums", |b| {
        b.iter(|| db.artist_albums(&some_artist).unwrap());
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
