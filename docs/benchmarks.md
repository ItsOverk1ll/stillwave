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
