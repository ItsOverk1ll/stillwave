# ADR 0016: A separate `stillwave-library` crate for scanning

Status: accepted (Phase 1)

## Context

Phase 1 needs a library scanner that reads tags (Lofty) and stores them. It fits none of the four Phase 0 crates: `stillwave-core` cannot depend on `stillwave-db` (the reverse dependency exists), and `stillwave-db` should not take on file-system walking and tag parsing. `CLAUDE.md` says to ask before deviating from the architecture; the maintainer chose a new crate.

## Decision

Add `crates/stillwave-library` (GPL-3.0). It depends on `stillwave-core`, `stillwave-db` and Lofty, walks folders, reads tags and properties, and calls `Db::import_tracks` in batches. It only ever reads audio files. The database side (`import_tracks`, `stored_files`, `remove_files`, schema v2 lookup indexes) lives in `stillwave-db`.

## Consequences

- Scanning is blocking and cancellable, meant for a worker thread; the UI never waits on it.
- Rescans skip files whose size and modified time match the database, so they are cheap.
- A vanished file loses its `media_files` row only. The media item stays, so playlists and likes survive an unplugged drive.
- One media item per file for now; merging duplicates is later work.
