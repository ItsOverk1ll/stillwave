# ADR 0018: Library list, scan and player wiring in the app

Status: accepted (Phase 1)

## Context

Phase 1 needs the app to scan a folder, show a 50,000-track library, search it as you type and play from it, without the UI ever waiting on the database or the disk.

## Decision

- The scanner talks to storage through a `LibraryStore` trait. `Db` implements it directly (tests, tools); the app implements it over its shared database and **locks per call**, so a long scan never freezes search or playback.
- The library list is paged and virtualised. Paging selects row IDs from an index first (`sort_key` for the library order, FTS rank for search) and joins only the page's rows; joining before paging made deep pages take over a second (see benchmarks.md).
- Search is debounced (120 ms) and cancels stale answers by remounting the list on every query or library change.
- The player bar is driven by polling `player_status` every 250 ms rather than pushing events; it is simple and cheap, and position only needs that resolution.
- The folder picker uses the official `tauri-plugin-dialog` (new dependency, small, maintained with Tauri).
- Audio device and mode are local settings (`meta` table, not change-logged) because they describe this device, not the user's library.
- Non-bit-perfect output is always labelled, with reasons, and the mixer fallback is an explicit setting, off by default.

## Consequences

- Rescans on startup are incremental, so a saved folder stays current without a button press.
- Polling means the badge can lag a track change by up to 250 ms.
- Mobile builds include the dialog plugin but cannot pick folders yet; playback returns an error off Windows.
