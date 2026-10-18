# Architecture

This describes what exists after Phase 0 and what the later phases will plug into. Anything not built yet is marked **planned**.

## Shape

```
          React + TypeScript UI (WebView)
                     |  Tauri commands (async, never blocking the UI thread)
        +------------+-------------------------------+
        |        apps/stillwave/src-tauri            |
        +--+------------+-------------+--------------+
           |            |             |
   stillwave-db   stillwave-audio   stillwave-plugins
           |            |             |
           +--- stillwave-core (types, IDs, clock) ---+
```

One Rust core is shared by desktop and mobile. The WebView draws the UI only; the audio path, the database and plugins live in Rust (and, on iPhone, a Swift plugin for the audio session).

## stillwave-core

Domain types (`MediaItem` with a `kind`, `Artist`, `Album`, `MediaFile`, `Playlist`, `PlaylistEntry`, `Placeholder`, `Play`, `Like`), UUIDv7 newtype IDs so a `PlaylistId` cannot be passed as a `TrackId`, a `DeviceId`, and a hybrid logical clock (`Hlc`) that orders changes across devices even when their clocks disagree.

## stillwave-db

SQLite via rusqlite (bundled, so FTS5 is always present), WAL mode, foreign keys on. Numbered migrations run in a transaction at startup; `PRAGMA user_version` records progress, and a database newer than the app is refused rather than damaged.

- Every user-data write goes through `Db::write`, which opens one transaction, applies the change, appends a `change_log` row (entity, operation, changed fields, HLC, device) and persists the clock. If anything fails, neither the data nor the log row is written.
- Search is an FTS5 index over title, artist and album kept current by triggers, with prefix indexes for search-as-you-type.
- Playlist entries and likes point at **either** a library item **or** a placeholder (enforced by a CHECK constraint), so an imported Spotify playlist keeps its full shape even for songs you do not own yet.
- Measured on the 50,000-track benchmark: search 1 to 8 ms (see benchmarks.md).

## stillwave-audio (interfaces only)

`Decoder` (file to interleaved left-aligned `i32` samples, never resampling), `OutputDevice` and `OutputStream` (opening a device must not resample; an unsupported format is an error the caller handles), and `SignalPath`, which records everything between file and device: source and output format, resampling, DSP, dither, software volume, exclusive mode, lossy source. `SignalPath::badge()` gives the UI's honest label (bit-perfect, processed, lossy).

Per-platform backends are empty modules behind `cfg` flags: WASAPI (Windows), ALSA/PipeWire (Linux), AAudio (Android), AVAudioSession (iOS). **Planned:** Phase 1 (desktop) and Phase 2 (mobile). Reference fixtures come from `scripts/gen_fixtures.py`; the hash-comparison harness in `tests/bit_perfect.rs` is ignored until a decoder exists.

## stillwave-plugins and plugin-sdk

See [plugins.md](plugins.md). Short version: signed WASM, permission manifest, only a few host functions, fuel and memory limits, no access to the audio path.

## Sync model (planned, Phase 4)

Each device keeps its full library locally and works offline. The `change_log` is the unit of sync: devices exchange log rows, order them by HLC and apply them; the device ID breaks ties deterministically. The optional account server stores only end-to-end encrypted data (Cloudflare Workers, D1, Durable Objects, R2). Nothing in Phase 0 talks to a network.

## App shell

`apps/stillwave` is Tauri 2. On start it opens `stillwave.db` in the OS app-data directory, and the placeholder home screen calls the `app_info` command (run on a blocking thread) to show app and core versions, schema version and database path. `gen/android` is committed (Tauri's recommendation); `gen/apple` can only be generated on macOS, so CI generates it (see ios-unsigned-builds.md).

## Verified vs not

Verified locally on Windows: all Rust tests, clippy, the frontend checks, the desktop app launching and showing real database values, the signed example plugin loading and being denied a permission it lacks. Not verified: anything on Linux, Raspberry Pi, Android (device), iOS, and the CI workflows themselves (they have never run).
