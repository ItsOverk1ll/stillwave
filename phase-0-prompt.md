# Stillwave — Phase 0 kickoff prompt

Paste everything below the line into Claude Code, in an empty folder that will become the `stillwave` repository. Put `CLAUDE.md` in that folder first.

---

You're starting Phase 0 (Foundations) of **Stillwave**, an open-source, audiophile-grade "super Spotify" for music people own. Read `CLAUDE.md` in this folder first and follow it throughout: it has the principles, hard rules, decisions and repo layout. Treat its decisions as settled; ask me before changing any of them.

## Goal of Phase 0

A clean, well-structured repository that builds and tests on every target platform in GitHub Actions, with the foundations every later phase depends on: the Rust workspace, the database schema built for sync, the plugin manifest and signing model, and an app shell that launches on desktop and mobile. **No playback, scanning or real features yet** — just a foundation that won't need to be torn up later.

## How to work

1. **Plan first.** Before writing code, check the tools available on this machine (Rust, Node, Tauri CLI, Android SDK/NDK; note what's missing and how to install it), then give me a short plan: the order you'll build things in, any decisions you need from me, and anything in this prompt you think is wrong. Wait for my OK.
2. **Build in small steps**, committing after each one, keeping everything building.
3. **Spike before committing to risky approaches** (unsigned iOS builds, Android packaging, Pi arm64 runners). Record findings in `docs/`.
4. At the end, give me a short report: what's done, what's verified, what you couldn't verify, and what Phase 1 should start with.

## Deliverables

### 1. Repository and workspace
- Git repo with the layout from `CLAUDE.md`, `.gitignore` (including `fixtures/private/`, build outputs, `.env*`), `.editorconfig`.
- `README.md`: what Stillwave is, the principles, platform list, how to build, project status ("pre-alpha").
- `CONTRIBUTING.md`: conventions from `CLAUDE.md`, the hard rules (no stream ripping, no DRM circumvention, nothing from userfy), how to run checks.
- Licence files: GPL-3.0 at the root and for `crates/` and `apps/`, MIT in `plugin-sdk/`, AGPL-3.0 in `server/`. SPDX headers in source files.
- Rust workspace (`Cargo.toml`) with the four crates; shared lints (`clippy` pedantic where reasonable), `rust-toolchain.toml` pinning a stable version.
- `docs/architecture.md` describing the shared core, per-platform audio outputs, plugin host and sync model, and `docs/decisions/` with one short ADR per decision in `CLAUDE.md`.

### 2. `stillwave-core`
- Domain types: `MediaItem` (generic, with a `kind`: track now; podcast episode, audiobook chapter, video later), `Artist`, `Album`, `MediaFile` (path, size, modified time, container/codec, sample rate, bit depth, channels, duration), `Playlist`, `PlaylistEntry`, `Placeholder` (a song not in the library: title, artist, album, duration, ISRC, external IDs such as Spotify URI), `Play`, `Like`.
- UUIDv7 IDs as newtypes (`TrackId`, `PlaylistId`, …) so they can't be mixed up.
- A `DeviceId` and a hybrid logical clock type for ordering changes across devices.
- Unit tests for ID generation, ordering and serialization.

### 3. `stillwave-db`
- SQLite via `rusqlite` (bundled, with FTS5), WAL mode, foreign keys on, busy timeout set.
- Versioned migrations run in a transaction at startup, with a test that applies all migrations to an empty database and another that checks a migrated database matches the expected schema.
- Tables for everything in `stillwave-core`, plus:
  - `change_log` (entity type, entity ID, operation, changed fields, HLC timestamp, device ID) written in the same transaction as every user-data change, for future sync;
  - an FTS5 index over titles, artists and albums, kept up to date by triggers;
  - playlist entries that can point to either a media item or a placeholder.
- A small repository API (insert/get/list/search) with tests, including a crash-safety test: kill a write mid-transaction (or simulate it) and confirm the database opens cleanly.
- A benchmark (Criterion) that inserts 50,000 synthetic tracks and measures search-as-you-type latency against the 50 ms target. Report the numbers; don't optimise yet.

### 4. `stillwave-audio` (interfaces only)
- Traits for a decoder and an output device, and a `SignalPath` type describing what happens between file and output (format, rate, bit depth, whether resampling/DSP/dither is active, whether the path is bit-perfect). This is what the UI's quality badge will show later.
- Empty per-platform backend modules behind `cfg` flags (WASAPI, ALSA/PipeWire, AAudio, iOS) so the structure exists.
- A fixtures script that generates reference audio (e.g. sine sweeps and silence-to-signal gapless test pairs) in FLAC, WAV and ALAC at 16/44.1, 24/96 and 24/192, plus a test harness skeleton that will compare decoded sample hashes. It can be `#[ignore]`d until Phase 1 adds decoding.

### 5. `stillwave-plugins`
- Plugin manifest format (TOML or JSON — pick one and justify it): ID, name, version, author, description, minimum app version, tier (`official` or `community`), and permissions (`network` with an explicit domain list, `microphone.clip`, `clipboard.read`, `files.read_selected`, `playlists.read`, `playlists.write`, `library.read`).
- Manifest parsing and validation with clear errors.
- Signature verification with ed25519: a plugin package is valid only if its signature checks against a trusted key, and `official` tier requires the Stillwave official key. Include a small dev tool or script to generate a keypair and sign a package. **Never commit a private key**; dev keys are generated locally and git-ignored.
- A WASM host skeleton (wasmtime on desktop) that loads a signed example plugin and calls one function, with permission checks enforced on host calls. Leave the iOS interpreter backend as a documented stub.
- Tests: valid plugin loads; tampered plugin, wrong key, or a community plugin claiming `official` are rejected; a plugin calling a host function it lacks permission for is denied.

### 6. `plugin-sdk` (MIT)
- A minimal Rust SDK crate for writing plugins, and one example plugin ("hello") that uses it, builds to WASM, and is signed in CI with a CI-only dev key.

### 7. `apps/stillwave` (Tauri 2 + React + TypeScript + Vite)
- App shell with a placeholder home screen showing: app name, version, the core crate version, the database schema version and the database path, fetched through a Tauri command (proving UI ↔ Rust ↔ DB works).
- Desktop targets: Windows, Linux. Mobile targets initialised: Android and iOS (`gen/` projects committed if that's Tauri's recommended practice; explain your choice).
- React set up with strict TypeScript, ESLint, Prettier, Vitest and one test. No UI library yet; plain CSS with variables for theming, light/dark aware.
- Background audio capability declared in the iOS config (`UIBackgroundModes: audio`) so it's in place for Phase 2.

### 8. GitHub Actions
- **Checks on every push and pull request:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, frontend lint + typecheck + tests.
- **Build matrix producing downloadable artifacts:**
  - Windows x64: installer (NSIS per-user and MSI)
  - Linux x64: `.deb`, `.rpm`, AppImage
  - Linux arm64 (for Raspberry Pi OS): `.deb` — use a native arm64 runner if available, otherwise cross-compile; document which
  - Android: debug-signed APK (arm64 at minimum)
  - iPhone: **unsigned `.ipa`** built on a macOS runner, suitable for SideStore to re-sign. Tauri's iOS build normally expects code signing; find the cleanest way to produce an unsigned build (for example building with code signing disabled and packaging the `.app` into `Payload/` inside a zip renamed `.ipa`). Spike this first and document it in `docs/ios-unsigned-builds.md`.
- Cache Rust and Node dependencies so CI stays fast.
- A placeholder release workflow that runs on tags and attaches all artifacts to a draft GitHub release.
- Leave an AUR `PKGBUILD` draft in `packaging/arch/` (publishing to the AUR comes later).

## Definition of done

- All checks and all platform builds pass in GitHub Actions; artifacts download and the desktop app opens on Windows and Linux, showing the placeholder screen with real values from the database.
- Database, ID, manifest, signature and permission tests pass; the 50,000-track search benchmark has been run and its result is recorded in `docs/benchmarks.md`.
- The iOS unsigned `.ipa` and Android APK are produced by CI, even if not yet tested on hardware.
- `README.md`, `CONTRIBUTING.md`, `docs/architecture.md` and the ADRs are written and match what was built.
- Your final report lists anything unverified and any decision I need to make before Phase 1.

## Out of scope for Phase 0

Playback, library scanning, real UI pages, Spotify import, accounts, the server, streaming plugins and recognition. Don't start them, even partially; stubs and interfaces only where listed above.
