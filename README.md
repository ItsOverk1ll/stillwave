# Stillwave

**Status: pre-alpha.** Phase 0 (foundations) is done and Phase 1 (desktop core) is partly built: on Windows the app can scan a music folder, list and search it, and play files through WASAPI with a signal-path badge. Nothing has been listened to on a real DAC yet, there is no import, and Linux, Android and iPhone do not play audio yet.

Stillwave is an open-source "super Spotify" for music you own: Spotify's comfort (synced playlists, mixes, radio, offline downloads, a yearly recap, device handoff) on top of bit-perfect playback of your own files. It runs on Windows, Linux, Raspberry Pi, Android and iPhone from one codebase, syncs through an optional account, and grows through sandboxed plugins.

## Principles (higher wins)

1. **Sound quality first.** The file as recorded leaves the app unless you choose otherwise: no silent resampling, no DSP by default, gapless always.
2. **Fast everywhere.** Cold start under 1 s, search-as-you-type under 50 ms on a 50,000-track library, 60 fps scrolling.
3. **Reliable.** A crash can never corrupt the library.
4. **Easy to switch to.** Moving from Spotify should take minutes.
5. **Local-first, account optional.** Everything works offline; synced data is end-to-end encrypted.
6. **Small core, plugins for the rest.** Optional features are signed WebAssembly plugins that never touch the audio path.

## Platforms

| Platform | Package | Phase 0 state |
| --- | --- | --- |
| Windows x64 | NSIS (per-user) and MSI | app runs (verified locally); installers built by CI |
| Linux x64 | deb, rpm, AppImage | built by CI only |
| Linux arm64 / Raspberry Pi OS | deb | built by CI only |
| Android | sideloaded APK | debug APK built by CI; not run on a device |
| iPhone | unsigned ipa for SideStore | ipa built by CI; not installed on a device |

"Built by CI" means the artifact exists but nobody has run it yet. See `docs/` for details.

## Layout

```
crates/stillwave-core     domain types, UUIDv7 IDs, hybrid logical clock
crates/stillwave-db       SQLite schema, migrations, change log, FTS5 search
crates/stillwave-audio    decoder, signal-path model, player, WASAPI output (Windows)
crates/stillwave-library  folder scanner: Lofty tags into the database
crates/stillwave-plugins  plugin manifest, ed25519 signing, WASM host
apps/stillwave            Tauri 2 app (React + TypeScript), desktop and mobile
plugin-sdk                MIT-licensed SDK and an example plugin
server                    account/sync service (placeholder until Phase 4)
docs                      architecture, decisions (ADRs), benchmarks, build notes
```

## Building

You need Rust (the version is pinned in `rust-toolchain.toml`), Node LTS and pnpm.

```bash
cargo test --workspace                  # all Rust tests
cd apps/stillwave
pnpm install
pnpm tauri dev                          # run the desktop app
pnpm tauri build                        # installers for the current OS
```

Linux also needs the Tauri system libraries (`libwebkit2gtk-4.1-dev`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`). Android needs JDK 17, the Android SDK and NDK 27, and on Windows, Developer Mode (Tauri creates symlinks). iOS builds only on macOS.

Checks (the same ones CI runs) are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

## Licences

GPL-3.0 for the apps and core crates, AGPL-3.0 for the account server, MIT for the plugin SDK. The choice is pending final confirmation.

Stillwave is not affiliated with Spotify, Tidal or any other service. It does not save audio from streaming services and does not circumvent DRM.
