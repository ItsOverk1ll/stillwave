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
