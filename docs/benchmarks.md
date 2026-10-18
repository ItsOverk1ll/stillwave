# Benchmarks

Reproduce with `cargo bench -p stillwave-db`. Numbers are a single run on the
development machine, not a CI guarantee; re-run on mid-range hardware before
Phase 1 sign-off.

## Library search (50,000 synthetic tracks, 2,000 artists)

Machine: Intel Core i9-12900K, Windows 11, in-memory SQLite (bundled, FTS5), release profile.
Search = FTS5 prefix match over title + artist + album, joined to `media_items`, 50 rows returned.

| Bulk insert (one transaction, incl. change log + FTS triggers) | 50,000 tracks in ~1.4 s |
| --- | --- |

| Query typed so far | Median latency | Budget |
| --- | --- | --- |
| `g` | 7.9 ms | 50 ms |
| `go` | 3.8 ms | 50 ms |
| `gol` | 3.8 ms | 50 ms |
| `golden` | 4.0 ms | 50 ms |
| `golden r` | 1.2 ms | 50 ms |
| `golden river` | 1.2 ms | 50 ms |

Result: well inside the 50 ms search-as-you-type budget, with no optimisation.

Caveats: in-memory database (an on-disk, cold-cache run will be slower), synthetic
vocabulary of 20 words (real libraries have a longer tail), a fast desktop CPU. The
single-letter query is the worst case. Not yet measured: on-disk cold start, Raspberry Pi.

## Library screen (50,000 imported tracks, on-disk database)

Machine and settings as above, except the database is a file (WAL) rather than in memory.
Run with `cargo bench -p stillwave-db --bench library`.

| Query | Median | Budget |
| --- | --- | --- |
| Page of 100 at the top of the library | 0.4 ms | 100 ms |
| Page of 100 at row 25,000 | 5.8 ms | 100 ms |
| Page of 100 at row 49,900 | 8.9 ms | 100 ms |
| Search "golden", first page of 100 (ranked) | 16 ms | 50 ms |
| Search "golden", count of matches | 0.6 ms | 50 ms |
| Import 50,000 tracks (batches of 250, tags already read) | about 14 s | n/a |

Lesson recorded: the first version joined artists, albums and files **before** applying `LIMIT/OFFSET`, so a page at row 25,000 took 615 ms and one at the end 1.25 s. Selecting the page's row IDs from an index first and joining only those rows brought deep pages to 6 to 9 ms. The library order comes from a stored, indexed `sort_key` (artist, album, disc, track, title).

Not measured: the time to read the tags of 50,000 real files from disk (the importer number excludes it), and a cold OS file cache.

## Albums and artists (same 50,000 tracks; worst case of one album per track)

| Query | Median | Budget |
| --- | --- | --- |
| Albums page of 100 at the top | 2.4 ms | 100 ms |
| Albums page of 100 at the end (50,000 albums) | 25 ms | 100 ms |
| Artists page of 100 at the end (2,000 artists) | 8 ms | 100 ms |
| One album's tracks | 0.006 ms | 100 ms |
| One artist's albums (own and "appears on") | 0.19 ms | 100 ms |

Lessons recorded:
- Without table statistics SQLite chose to compute each album's codec and quality by walking every file on the device through the `(device, path)` index: an album page took **12 to 16 s** and an artist's albums 6.5 s. `CROSS JOIN` now pins the order (album's tracks, then their files) and the app runs `PRAGMA optimize` after every scan.
- Sorting albums by a joined artist name on every page cost 164 ms at the end of the list. Albums now carry an indexed `sort_key` kept current by SQL triggers (schema v5), the same approach as tracks.
