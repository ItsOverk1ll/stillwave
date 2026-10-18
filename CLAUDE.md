# Stillwave — project guide for Claude Code

Stillwave is an open-source "super Spotify" for music you own: Spotify's comfort (synced playlists, mixes, radio, offline downloads, a yearly recap, device handoff) on top of bit-perfect playback of your own files. It runs on Windows, Linux, Raspberry Pi, Android and iPhone from one codebase, syncs through an optional account, and grows through sandboxed plugins.

The full plan lives in the Stillwave Project Plan doc. This file is the short version you must follow in every session.

## Core principles (in priority order — higher wins when they conflict)

1. **Sound quality first.** What leaves the app is the file as recorded unless the user chooses otherwise. No silent resampling, no lossy shortcuts, no DSP on by default ("Pure mode"). Any processing runs in 64-bit float with headroom; TPDF dither when reducing bit depth; gapless is a requirement.
2. **Fast everywhere.** Targets (50,000-track library, mid-range PC): cold start < 1 s, play-to-sound < 150 ms, search-as-you-type < 50 ms, page open < 100 ms, 60 fps scrolling, < 200 MB idle. Speed never comes from touching the audio path.
3. **Reliable.** Playback never stutters, scanning never blocks the UI, a crash can never corrupt the library (SQLite WAL, transactions, migrations tested).
4. **Easy to switch to.** Moving from Spotify should take minutes (paste, drag-and-drop, data-export import).
5. **Local-first, account optional.** Everything works offline with no account. Synced data is end-to-end encrypted.
6. **Small core, plugins for the rest.** Optional or risky features are WebAssembly plugins with declared permissions. Plugins never touch the playback audio path.

## Hard rules

- **Nothing from userfy.** It is closed source and only an inspiration. Do not copy, decompile, or imitate its code, assets, names or design.
- **No stream ripping.** Never build features that save audio from streaming services (Spotify, Tidal, YouTube, etc.) or download from file-sharing networks. Songs a user doesn't own come from official streaming SDKs (played, never saved), free legal sources (Jamendo, Internet Archive), or stores.
- **No DRM circumvention, ever**, including in plugins, examples, tests or scripts.
- **Never commit personal data.** Spotify exports, real libraries and test playlists go in `fixtures/private/` (git-ignored).
- **Never mutate a user's audio files.** Tag writing is opt-in and metadata-only.
- **Ask before** adding a heavy dependency, changing a decision listed below, or deviating from the architecture.

## Decisions already made (do not relitigate without asking)

| Area | Decision |
| --- | --- |
| Name | Stillwave |
| Licence | GPL-3.0 for the apps and core crates, AGPL-3.0 for the account server, MIT for the plugin SDK (pending final confirmation — keep licence files easy to change) |
| App shell | Tauri 2, one app targeting desktop and mobile |
| UI | React + TypeScript on Vite |
| Core | Rust workspace; SQLite (WAL, FTS5); Lofty for tags; Symphonia for decoding |
| Audio output | Per platform: WASAPI (Windows, exclusive + shared), ALSA direct / PipeWire (Linux, Pi), AAudio (Android, bit-perfect USB on 14+), native AVAudioSession via a Swift plugin (iPhone) |
| Plugins | WebAssembly with a permissions manifest; signed (ed25519); full-speed runtime on desktop, interpreter on iOS |
| IDs | UUIDv7 for every entity, plus a change log, so devices can sync and merge |
| Schema | Generic "media items" (music first; podcasts, audiobooks, video later) |
| Builds | GitHub Actions for every platform; native hardware testing later |
| iPhone distribution | Unsigned `.ipa`, sideloaded with SideStore (users sign with their own Apple ID) |
| Android distribution | APK sideload; Play Store optional later |
| Linux packages | `.deb` (Debian, Ubuntu, Raspberry Pi OS arm64), `.rpm` (Fedora), AUR (Arch), AppImage/Flatpak as a catch-all |
| Server (later) | Cloudflare Workers, D1, Durable Objects, R2 |

## Repository layout

```
stillwave/
├── CLAUDE.md
├── README.md, CONTRIBUTING.md, LICENSE (GPL-3.0)
├── Cargo.toml                 # Rust workspace
├── crates/
│   ├── stillwave-core/        # domain types, IDs, library logic
│   ├── stillwave-db/          # SQLite schema, migrations, queries, FTS
│   ├── stillwave-audio/       # decode + output traits, per-platform backends
│   └── stillwave-plugins/     # WASM host, manifest, permissions, signature checks
├── apps/stillwave/            # Tauri 2 app (desktop + Android + iOS)
│   ├── src/                   # React + TypeScript UI
│   └── src-tauri/             # Rust side of the app, commands, mobile gen/
├── plugin-sdk/                # MIT-licensed SDK + example plugin
├── server/                    # AGPL-3.0, account/sync service (placeholder until Phase 4)
├── fixtures/                  # generated test audio; fixtures/private/ is git-ignored
├── scripts/                   # fixture generation, plugin packaging, unsigned iOS build
├── packaging/arch/            # AUR PKGBUILD draft
├── docs/
│   ├── architecture.md
│   └── decisions/             # one short ADR per decision
└── .github/workflows/
```

## Working conventions

- Small, focused commits with clear messages. Keep `main` building on every platform.
- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` must pass. No `unwrap()` in non-test code paths that handle user data or files.
- TypeScript: strict mode, ESLint, Vitest.
- Heavy work (scanning, decoding, DB queries) never runs on the UI thread.
- Every new decision gets a short ADR in `docs/decisions/` and, if it changes this file, an update here.
- When unsure about a platform behaviour (iOS signing, Android audio, WebView DRM), write a small spike, record the finding in `docs/`, then build.
- Prefer saying "I couldn't verify X" over guessing.

## Roadmap

0. Foundations — repo, workspace, schema, plugin signing, CI for every platform
1. Desktop core — Windows + Linux + Pi, scanner, bit-perfect playback, library views
2. Mobile core — iPhone (SideStore) + Android, native audio, file import
3. Spotify switch — paste/drag import, data-export import, likes, playlists, placeholders
4. Accounts and sync — optional account, per-device downloads, streaming from home
5. Made For You — mixes, radio, stats, yearly recap, Find Song plugin
6. Polish — device handoff, themes, EQ, plugin manager
7. Community — plugin registry, author signing, review, kill switch (gate: polished and secure)

Every release must pass the bit-perfect tests and the performance budget.
