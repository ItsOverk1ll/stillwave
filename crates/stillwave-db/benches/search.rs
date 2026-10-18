// SPDX-License-Identifier: GPL-3.0-only
//! Inserts 50,000 synthetic tracks, then measures search-as-you-type latency
//! (target: < 50 ms). Run with `cargo bench -p stillwave-db`.

use criterion::{criterion_group, criterion_main, Criterion};
use stillwave_core::{Artist, ArtistId, ExternalIds, MediaItem, MediaItemId, MediaKind};
use stillwave_db::Db;

const WORDS: &[&str] = &[
    "night", "river", "golden", "echo", "paper", "shadow", "ocean", "neon", "velvet", "static",
    "winter", "signal", "garden", "mirror", "thunder", "silver", "dream", "highway", "ember",
    "tide",
];

fn seed(db: &mut Db) {
    let artists: Vec<Artist> = (0..2_000)
        .map(|n| Artist {
            id: ArtistId::new(),
            name: format!("{} {} {n}", WORDS[n % 20], WORDS[(n / 20) % 20]),
            sort_name: None,
            external_ids: ExternalIds::new(),
        })
        .collect();
    for a in &artists {
        db.insert_artist(a).unwrap();
    }
    let items: Vec<MediaItem> = (0..50_000usize)
        .map(|n| MediaItem {
            id: MediaItemId::new(),
            kind: MediaKind::Track,
            title: format!("{} {} {n}", WORDS[n % 20], WORDS[(n * 7 + 3) % 20]),
            artist_id: Some(artists[n % artists.len()].id),
            album_id: None,
            track_number: Some(u32::try_from(n % 14).unwrap_or(0) + 1),
            disc_number: None,
            duration_ms: Some(180_000),
            isrc: None,
            external_ids: ExternalIds::new(),
        })
        .collect();
    let start = std::time::Instant::now();
    db.insert_media_items(&items).unwrap();
    eprintln!("inserted 50,000 tracks in {:?}", start.elapsed());
}

fn bench(c: &mut Criterion) {
    let mut db = Db::open_in_memory().unwrap();
    seed(&mut db);
    let mut group = c.benchmark_group("search_50k");
    // Each keystroke of "golden river" as the user types it.
    for q in ["g", "go", "gol", "golden", "golden r", "golden river"] {
        group.bench_function(q, |b| b.iter(|| db.search(q, 50).unwrap()));
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
